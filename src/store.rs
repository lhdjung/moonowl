//! What the reader remembers between runs: the settings table, and the themes
//! it is choosing from.
//!
//! [`crate::settings`] and [`crate::theme`] are the app's own modules, mounted
//! unchanged, and they are about the disk and nothing else. This is the layer
//! above them the reader talks to: which theme is in use, what it resolves to,
//! and a way to change a setting that writes it down.
//!
//! **There is no bridge here**, which is what this replaces: `api.ts` (898
//! lines), thirty-three commands, a browser twin of each, and
//! `settings.test.mjs` existing solely because the table is written out three
//! times. The table is stated once, in the file the app states it in.
//!
//! *Every write is off the main thread.* Where the reader *is* moves sixty
//! times a second, and a whole-file rewrite of `library.toml` landing in the
//! middle of a scroll is the one gesture this app exists to make smooth.
//! `Scribe` is one thread with one pending place per document, written when
//! the scrolling stops — per document because `cargo test` runs tests in
//! parallel and a single slot would have one test's position replacing
//! another's, intermittently. The small writes — a theme, a mark, the
//! journal — go down the same channel as [`Job::Now`] and land in order,
//! memory changed first. `library::touch` at open is the one exception: it is
//! the read, and the one place an unwritable library is reported.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use serde_json::{json, Value};

/// What a mark kept beside the document is written down as being: the app's
/// own `MARKUP_OPACITY`, so a journal this reader writes and the app reads
/// says what the app would have said. Nothing here draws with it — a mark in
/// the file is drawn by pdfium out of the appearance stream pdfium generates.
const MARKUP_OPACITY: f64 = 0.35;

use crate::emit::Post;
use crate::keys;
use crate::layout::Anchor;
use crate::library::{self, Highlight, Mark};
use crate::palette::{self, Palette};
use crate::settings::{self, Settings};
use crate::theme;

/// How long the scrolling has to stop for before where the reader is gets
/// written down. `onScroll` in `main.ts` is the same number, spelled
/// `setTimeout(… , 700)`.
const SETTLE: Duration = Duration::from_millis(700);

/// The one thread that writes down where the reader is, and everything else
/// this reader writes to its own directory.
///
/// **The write that made it necessary is the place, and the reason is the
/// rate.** A theme is chosen a few times a session; the scroll offset changes
/// on every wheel event, and every change is a read-modify-write of the whole
/// of `library.toml`.
///
/// Two things it does: moves the write off the thread that is scrolling, and
/// *coalesces* — a place arriving while another is pending replaces it, and
/// nothing is written until [`SETTLE`] has passed with nothing new. So a reader
/// scrolling through a chapter costs one write rather than four hundred.
///
/// **Pending places are keyed by document**, for one reason: `cargo test` runs
/// its tests in parallel and this thread is the process's. A single slot would
/// have one test's position replacing another's, intermittently and by
/// timing.
struct Scribe {
    jobs: Sender<Job>,
}

enum Job {
    /// Where the reader is in one document, superseding whatever was pending
    /// for it.
    Place {
        dir: PathBuf,
        file: String,
        page: u32,
        offset: f64,
        label: String,
    },
    /// A setting that moves continuously — the zoom during a pinch — held
    /// until it stops moving. `App.setSoon` in `main.ts`, and the same
    /// reason: one write per frame of a gesture is one whole-file rewrite per
    /// frame, and only the value the gesture ends on matters.
    Setting {
        dir: PathBuf,
        key: String,
        value: Value,
    },
    /// A setting written outright in the meantime: what was pending for it is
    /// older and must not land on top. A pinch and then ⌘0 inside the wait
    /// left `fit_mode = "actual"` on the disk under a window fitted to width.
    Forget { dir: PathBuf, key: String },
    /// Write everything pending now and say when it is done. What quitting
    /// asks for, and what a test asks for instead of sleeping.
    Flush(Sender<()>),
    /// A small write done as soon as the thread gets to it: a mark, a theme,
    /// the journal. Memory is changed on the main thread first and the disk
    /// follows, in order, so nothing that touches the disk is on the thread
    /// drawing the window.
    Now(Box<dyn FnOnce() + Send>),
}

/// Hand a write to the scribe: run as it arrives, in order with the rest,
/// and off the thread that draws the window.
pub fn later(write: impl FnOnce() + Send + 'static) {
    let _ = Scribe::get().jobs.send(Job::Now(Box::new(write)));
}

impl Scribe {
    /// The process's, made on first use. There is no way to stop it and
    /// nothing that would want to: it is asleep on a channel except when
    /// there is something to write.
    fn get() -> &'static Scribe {
        static SCRIBE: OnceLock<Scribe> = OnceLock::new();
        SCRIBE.get_or_init(|| {
            let (jobs, inbox) = mpsc::channel();
            std::thread::Builder::new()
                .name("moonowl-library".into())
                .spawn(move || run(inbox))
                .expect("a thread to write the library on");
            Scribe { jobs }
        })
    }
}

/// Wait, coalesce, write.
///
/// The shape is `clearTimeout` and `setTimeout` from `main.ts` turned inside
/// out: with nothing pending this blocks for ever, and with something pending
/// it waits [`SETTLE`] for a newer answer and writes when none comes. A place
/// arriving in the meantime restarts the wait, which is what makes a
/// continuous scroll cost one write — and means, exactly as in the app, that
/// a scroll which never pauses is not written down until something asks for a
/// flush.
fn run(inbox: Receiver<Job>) {
    let mut pending: BTreeMap<(PathBuf, String), (u32, f64, String)> = BTreeMap::new();
    let mut settings_pending: BTreeMap<(PathBuf, String), Value> = BTreeMap::new();
    loop {
        let job = if pending.is_empty() && settings_pending.is_empty() {
            inbox.recv().map_err(|_| RecvTimeoutError::Disconnected)
        } else {
            inbox.recv_timeout(SETTLE)
        };
        match job {
            Ok(Job::Place {
                dir,
                file,
                page,
                offset,
                label,
            }) => {
                pending.insert((dir, file), (page, offset, label));
            }
            Ok(Job::Setting { dir, key, value }) => {
                settings_pending.insert((dir, key), value);
            }
            Ok(Job::Forget { dir, key }) => {
                settings_pending.remove(&(dir, key));
            }
            Ok(Job::Now(write)) => write(),
            Ok(Job::Flush(done)) => {
                write_out(&mut pending);
                write_settings(&mut settings_pending);
                // The sender may be gone — a test that stopped waiting — and
                // that is not this thread's problem.
                let _ = done.send(());
            }
            Err(RecvTimeoutError::Timeout) => {
                write_out(&mut pending);
                write_settings(&mut settings_pending);
            }
            Err(RecvTimeoutError::Disconnected) => {
                write_out(&mut pending);
                write_settings(&mut settings_pending);
                return;
            }
        }
    }
}

fn write_out(pending: &mut BTreeMap<(PathBuf, String), (u32, f64, String)>) {
    for ((dir, file), (page, offset, label)) in std::mem::take(pending) {
        // A library that cannot be written is a reader who loses their place,
        // which is worth nothing at all on a thread with nowhere to say it.
        // The notice for that case is raised at open, where the same file is
        // written by `touch` and somebody is looking at the screen.
        refused(&dir, library::remember(&dir, &file, page, offset, &label));
    }
}

/// The settings the scribe is holding, written a directory at a time so that
/// a pinch and a theme chosen in the same second are one rewrite rather than
/// two.
fn write_settings(pending: &mut BTreeMap<(PathBuf, String), Value>) {
    let mut by_dir: BTreeMap<PathBuf, Vec<(String, Value)>> = BTreeMap::new();
    for ((dir, key), value) in std::mem::take(pending) {
        by_dir.entry(dir).or_default().push((key, value));
    }
    for (dir, entries) in by_dir {
        refused(&dir, settings::set_many(&dir, entries));
    }
}

/// Write down everything the scribe is holding, and wait for it.
///
/// Called on the way out — `main.rs`, once the event loop has returned — and
/// by any test that wants to reopen a reader and find its place kept. Without
/// it a run that ends while somebody is still scrolling loses the last
/// seven hundred milliseconds of reading, which is a page.
pub fn flush() {
    let (done, wait) = mpsc::channel();
    if Scribe::get().jobs.send(Job::Flush(done)).is_ok() {
        // Two seconds is not a timeout anybody should reach; it is here so
        // that a thread which has somehow died cannot hold up a quit.
        let _ = wait.recv_timeout(Duration::from_secs(2));
    }
}

/// Whether a document's own `/Title` is worth calling it by.
///
/// `worthCalling` in `main.ts`, and every line of it is a fact about what
/// producers actually write rather than about this app. A great many PDFs
/// carry a title filled in by the program that made them and not by anybody:
/// the file name again, the file name of the *source* — "Microsoft Word -
/// report.doc" — or the word "untitled". Each of those is worse than the file
/// name, because it looks deliberate. Anything that fails leaves the file name
/// alone, which is what it was before.
pub fn worth_calling(title: &str, file_name: &str) -> bool {
    let title = title.trim();
    if title.chars().count() < 4 || title.chars().count() > 200 {
        return false;
    }
    let folded = title.to_lowercase();
    let name = file_name.to_lowercase();
    let stem = name.strip_suffix(".pdf").unwrap_or(&name);
    if folded == stem || folded == name {
        return false;
    }
    if folded.starts_with("untitled")
        && !folded[8..].starts_with(|c: char| c.is_alphanumeric() || c == '_')
    {
        return false;
    }
    if folded.starts_with("microsoft word -") {
        return false;
    }
    if let Some(rest) = folded.strip_prefix("document") {
        if rest.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }
    }
    // A title that is a file name is a file name, whatever file it names.
    const SUFFIXES: &[&str] = &[
        ".pdf", ".doc", ".docx", ".tex", ".indd", ".ppt", ".pptx", ".odt", ".rtf", ".ps", ".dvi",
    ];
    if SUFFIXES.iter().any(|suffix| folded.ends_with(suffix)) {
        return false;
    }
    true
}

/// What to call a document: its own `/Title` where that is worth having, and
/// the file's name where it is not.
///
/// A function rather than a method for the reason [`reopening`] is one: a
/// window is given its title before there is a [`Store`] to ask, because a
/// window's title is an attribute handed to the builder. `Store::opened`
/// decides the same thing the same way, and this is the deciding.
pub fn called(path: &str, declared: &str) -> String {
    let name = std::path::Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string());
    if worth_calling(declared, &name) {
        declared.trim().to_string()
    } else {
        name
    }
}

/// A document's file name, which is what the shelf calls it when the document
/// itself says nothing worth using.
/// Whether two readings of the journal say the same thing. `at` is left out:
/// an entry rebuilt from the file is stamped with the time it was read, and
/// a journal that differed only in that would be written on every reload.
fn same_journal(a: &[Highlight], b: &[Highlight]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            x.id == y.id
                && x.page == y.page
                && x.quads == y.quads
                && x.color == y.color
                && x.quote == y.quote
                && x.annotation_id == y.annotation_id
        })
}

fn file_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

/// One row of the recently-read list: what to call it, where it is, and where
/// the reader stopped.
///
/// The name is settled here rather than at the row, because the library
/// already holds what [`called`] decided when the document was opened and a
/// row that worked it out again would be a second opinion about the same
/// question. An entry written before this reader knew about titles has an
/// empty one, which is why the fallback is here at all.
#[derive(Clone, Debug, PartialEq)]
pub struct Recent {
    pub path: String,
    pub title: String,
    pub page: usize,
    /// What that page is called, as the toolbar said it: the page's number
    /// where the document gives none.
    pub label: String,
}

/// What was open when the reader was last put down, if it is still there and
/// the reader wants it back.
///
/// Read before there is a window, which is why it is a function rather than a
/// method: `main.rs` has to know what to open before it can make a [`Store`].
///
/// `prune` keeps it honest — a document that has been moved or deleted would
/// otherwise be reopened and fail on every launch for ever. And
/// `reopen_last_document` is asked *here* rather than left to the caller,
/// because two sides that each assume the other checked it are two sides that
/// disagree about whether the window has anything in it.
pub fn reopening(dir: &Path) -> Option<String> {
    let settings = settings::load(dir);
    let wanted = settings
        .get("reopen_last_document")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if !wanted {
        return None;
    }
    // **One window, on the document read most recently.** `library.open` is a
    // list because there were several windows and every one of them came back;
    // a cascade of five windows, the last of them off the bottom-right corner
    // of the screen, is not what anybody meant by "pick up where I left off".
    // The list is still written and still pruned — it is what says which of
    // several documents was actually open — but only one of them is opened,
    // and `opened_at` is what says which. Order in the list is the order the
    // windows were *made*, which is the wrong end of it.
    let library = library::prune(&library::load(dir));
    library
        .open
        .iter()
        .max_by_key(|path| {
            library
                .files
                .iter()
                .find(|entry| &&entry.path == path)
                .map(|entry| entry.opened_at)
                .unwrap_or(i64::MIN)
        })
        .cloned()
}

/// What wearing a theme did, beyond putting it on.
///
/// The name is for the notice line, and the flag is the sentence said beside
/// it: a theme worn against the machine while following it holds only until
/// the machine next switches, and the reader has to be told so. See
/// [`Store::wear`].
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct Worn {
    pub name: String,
    pub overruled: bool,
}

/// The settings as every window of this process holds them, per directory.
///
/// **One table, not one per window.** Each window read its own copy at
/// launch, so a window that had not seen a change wrote its stale value back
/// over it: Nord chosen in one tab, ⌘D in the other, and the dark slot was
/// Moonowl Dark again. Held weakly: once no window is using a directory, the
/// next one reads the disk afresh.
fn shared(dir: &Path) -> Arc<std::sync::Mutex<Settings>> {
    type Live = std::collections::HashMap<PathBuf, std::sync::Weak<std::sync::Mutex<Settings>>>;
    static LIVE: OnceLock<std::sync::Mutex<Live>> = OnceLock::new();
    let mut live = LIVE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(held) = live.get(dir).and_then(std::sync::Weak::upgrade) {
        return held;
    }
    let held = Arc::new(std::sync::Mutex::new(settings::load(dir)));
    live.insert(dir.to_path_buf(), Arc::downgrade(&held));
    held
}

/// Every window reading a settings directory, to be told when the theme
/// changes under it: the table is shared (see [`shared`]), but a window only
/// paints what it has when something makes it render.
fn listeners() -> std::sync::MutexGuard<'static, std::collections::HashMap<PathBuf, Vec<Post>>> {
    static LISTENERS: OnceLock<std::sync::Mutex<std::collections::HashMap<PathBuf, Vec<Post>>>> =
        OnceLock::new();
    LISTENERS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Send news to every window reading a settings directory. Collected, then
/// sent with the lock let go: a waker may run the task inline, and that task
/// may take this lock.
fn tell(dir: &Path, event: &str, payload: crate::emit::Payload) {
    let posts: Vec<Post> = listeners()
        .get_mut(dir)
        .map(|posts| {
            posts.retain(Post::read_by_anyone);
            posts.clone()
        })
        .unwrap_or_default();
    for post in posts {
        post.send(crate::emit::News {
            event: event.into(),
            target: None,
            payload: payload.clone(),
        });
    }
}

/// **A write the disk refused is said, once.** These run on the scribe's
/// thread, and every one of them was `let _`: a settings or library file
/// broken by hand while the app ran made every later change vanish without a
/// word, and the next launch undid the lot. Said again only after a write
/// has succeeded, so a reader scrolling is not told every 700ms.
fn refused<T>(dir: &Path, written: Result<T, String>) {
    static SAID: OnceLock<std::sync::Mutex<std::collections::HashMap<PathBuf, String>>> =
        OnceLock::new();
    let mut said = SAID
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    match written {
        Ok(_) => {
            said.remove(dir);
        }
        Err(why) if said.get(dir) != Some(&why) => {
            said.insert(dir.to_path_buf(), why.clone());
            drop(said);
            tell(dir, "disk-refused", crate::emit::Payload::Text(why));
        }
        Err(_) => {}
    }
}

/// How a machine's appearance is written down: "dark", "light", or nothing.
fn darkness(dark: Option<bool>) -> &'static str {
    match dark {
        Some(true) => "dark",
        Some(false) => "light",
        None => "",
    }
}

pub struct Store {
    dir: PathBuf,
    themes_dir: PathBuf,
    settings: Arc<std::sync::Mutex<Settings>>,
    themes: Vec<theme::Theme>,
    /// A theme chosen for this run and not written down, which is what
    /// `--theme` is. A flag that quietly rewrote a setting would be a flag
    /// that changes what the *next* run does, which is not what a flag means.
    for_now: Option<usize>,
    /// Colours a theme names that the renderer cannot read, if any — raised
    /// once, as the app's `unreadableColors` notice is.
    pub complaint: Option<String>,
    /// The document this reader has open, as the library names one: the path,
    /// as given. Empty until [`Store::opened`] is called, which is what puts
    /// the document into `library.toml` and is the only reason a mark has
    /// anywhere to go.
    file: String,
    /// The pins in that document, kept in memory so that drawing the sidebar
    /// is not a read of a file per frame. The file is still the record —
    /// every change here is written through, and the write is what the next
    /// run reads.
    marks: Vec<Mark>,
    /// Markup kept beside the document because it could not go into it. See
    /// [`Store::journal`].
    journal: Vec<Highlight>,
    /// How many times the journal has been written since the store was made.
    /// See [`Store::journal_rev`].
    journal_rev: u64,
    /// The shelf as last read, with the library file's modification time it
    /// was read at. See [`Store::recents`].
    recents: std::cell::RefCell<Option<(Option<std::time::SystemTime>, Vec<Recent>)>>,
    /// What the document is called on the shelf: its own `/Title` where that
    /// is worth having, and the file's name where it is not. Decided once, at
    /// open, by [`worth_calling`].
    title: String,
    /// Whether the machine is in dark mode, as the window reports it —
    /// `None` where the platform does not say, which winit allows and two of
    /// the three platforms have historically been.
    ///
    /// `darkOutside()` in `main.ts` is `matchMedia("(prefers-color-scheme:
    /// dark)")` and always answers, because a webview is a browser. Here the
    /// answer can be absent, and absent is not "light": a reader whose
    /// machine will not say must be left wearing what they chose rather than
    /// moved to the light theme every launch. So every rule below is written
    /// against `Some`, and `None` means the two switches simply do nothing.
    outside: Option<bool>,
}

impl Store {
    /// The reader's own directory, with the shipped themes written into it.
    pub fn open() -> Store {
        Store::at(&crate::config::config_dir())
    }

    /// One stated directory, which is what a test has and what
    /// `MOONOWL_CONFIG` gives a run.
    pub fn at(dir: &Path) -> Store {
        let themes_dir = dir.join("themes");
        // On every run, so that a shipped theme whose colours change reaches a
        // machine that already has the old one. See the comment on the
        // function: a built-in edited in place is overwritten, deliberately,
        // and every shipped file carries a banner saying so.
        theme::install_built_ins(&themes_dir);
        let themes = theme::load_all(&themes_dir);
        // Once, and then never again: unlike a shipped theme this file is the
        // reader's from the moment it exists, and every line of the template
        // is a comment. `keys::install` is the app's own and says why.
        keys::install(dir);
        // A window opened a moment after a setting changed reads what that
        // change wrote, not what it is about to write.
        flush();
        let mut store = Store {
            settings: shared(dir),
            dir: dir.to_path_buf(),
            themes_dir,
            themes,
            for_now: None,
            complaint: None,
            file: String::new(),
            marks: Vec::new(),
            journal: Vec::new(),
            journal_rev: 0,
            recents: std::cell::RefCell::new(None),
            title: String::new(),
            outside: None,
        };
        store.complaint = settings::problem(dir).or_else(|| store.unreadable());
        store
    }

    /// What `keys.toml` says, and the lines of it that were not usable.
    ///
    /// Read rather than held, because the one thing that will want it twice
    /// is a Reload button — the app has one on its Keyboard page, for the
    /// reason `keys.rs` gives: this directory is written to several times a
    /// minute while somebody is scrolling, so a watcher over it would be
    /// answering its own writes.
    ///
    /// The bindings are carried across as written. What an action *name*
    /// means, and whether a chord can be read at all, is
    /// [`crate::keymap`]'s — which is the same split the app has across its
    /// bridge, and it did not have to move to get here.
    pub fn keyboard(&self) -> keys::Keys {
        keys::load(&self.dir)
    }

    pub fn themes(&self) -> &[theme::Theme] {
        &self.themes
    }

    /// The themes directory, which the settings window and the watcher will
    /// both want and neither exists yet.
    pub fn themes_dir(&self) -> &Path {
        &self.themes_dir
    }

    /// Where `settings.toml` and `keys.toml` live — the About page names it,
    /// because a reader who is told their settings are a plain file is owed
    /// the path to it.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Where the theme in use sits in [`Store::themes`].
    ///
    /// A theme is remembered by **id**, not by position: the list changes when
    /// somebody adds a file to the directory, and a position would then mean a
    /// different theme than it did yesterday. A id naming nothing falls back
    /// to the default light theme rather than to nothing, which is what makes
    /// deleting the theme you are wearing survivable.
    pub fn theme_index(&self) -> usize {
        if let Some(index) = self.for_now.filter(|&index| index < self.themes.len()) {
            return index;
        }
        let wanted = self.text("theme");
        self.themes
            .iter()
            .position(|theme| theme.id == wanted)
            .or_else(|| {
                self.themes
                    .iter()
                    .position(|theme| theme.id == theme::DEFAULT_LIGHT)
            })
            .unwrap_or(0)
    }

    pub fn theme(&self) -> &theme::Theme {
        &self.themes[self.theme_index()]
    }

    /// The theme in use, as colours. `recolor_images` is the setting that says
    /// whether a pixel with a colour of its own keeps it, which is why it is
    /// read here rather than off the theme.
    pub fn palette(&self) -> Palette {
        palette::resolve(self.theme(), self.flag("recolor_images"))
    }

    /// Wear a theme, by its place in the list.
    ///
    /// Two settings move together, which is what `set_many` is for: the theme
    /// in use, and the light or dark slot it fills — so that following the
    /// machine's appearance later comes back to the theme somebody chose for
    /// that half rather than to the shipped default. Which slot it fills is
    /// read off the theme's own paper, because that is the only thing that
    /// actually makes a theme dark.
    ///
    /// **No third setting moves with them.** Choosing a theme whose darkness
    /// disagrees with the machine while following it used to switch following
    /// off, and the brief has every setting stand on its own. The choice holds
    /// instead until the machine next switches — see [`overruled`] — and the
    /// [`Worn`] says so, for the notice line.
    pub fn wear(&mut self, index: usize) -> Worn {
        let Some(theme) = self.themes.get(index) else {
            return Worn::default();
        };
        // The theme editor's unsaved draft stands in the list with no id, and
        // choosing it from the menu or with `t` wrote `theme = ""` — the next
        // launch fell back to the default and the light/dark pair was lost.
        // It is worn as the editor wears it, and nothing is written.
        if theme.id.trim().is_empty() {
            let name = theme.name.clone();
            self.wear_for_now(index);
            return Worn {
                name,
                overruled: false,
            };
        }
        // Choosing one by hand is the reader deciding, which outranks a flag
        // and is written down.
        self.for_now = None;
        let (id, name) = (theme.id.clone(), theme.name.clone());
        let dark = self.is_dark(theme);
        let slot = if dark { "dark_theme" } else { "light_theme" };
        let mut moving = vec![("theme".into(), json!(id)), (slot.into(), json!(id))];
        if theme.built_in {
            let last = if dark {
                "last_built_in_dark"
            } else {
                "last_built_in_light"
            };
            moving.push((last.into(), json!(id)));
        }
        let against = self
            .outside
            .filter(|&outside| self.flag("follow_system_theme") && outside != dark);
        moving.push(("theme_chosen_against".into(), json!(darkness(against))));
        self.set(moving);
        tell(&self.dir, "theme-worn", crate::emit::Payload::Nothing);
        self.complaint = self.unreadable();
        Worn {
            name,
            overruled: against.is_some(),
        }
    }

    /// What the machine says about light and dark, and `None` where it will
    /// not say. See [`Store::outside`] — written whenever the window reports
    /// it, which is at startup and on every change.
    /// Tell this window when another one changes the theme. See
    /// [`listeners`].
    pub fn listen(&self, post: Post) {
        listeners().entry(self.dir.clone()).or_default().push(post);
    }

    pub fn set_outside(&mut self, dark: Option<bool>) {
        self.outside = dark;
    }

    pub fn outside(&self) -> Option<bool> {
        self.outside
    }

    /// Whether the theme in use is a dark one, which is read off its paper.
    pub fn dark_now(&self) -> bool {
        self.is_dark(self.theme())
    }

    /// Which theme fills the light or the dark half of the pair.
    ///
    /// `toggleDark` in `main.ts`: the theme remembered for that half if it is
    /// still there, else anything of that darkness. Nothing here decides
    /// *which* light theme or *which* dark one — those are two settings the
    /// reader has already chosen, and this only says which of them is in
    /// force. So somebody with Sepia by day and Tokyo Night by night gets
    /// exactly that pair, and somebody who has never thought about it gets
    /// the shipped defaults.
    ///
    /// `None` only where there is no theme of that darkness at all, which
    /// takes deleting eight files.
    ///
    /// One line more than the app's, and deliberately: the slot is checked
    /// for being of the darkness it is the slot *for*. A theme file whose
    /// paper was edited from dark to light is still named by `dark_theme`,
    /// and the app trusts the name — so ⌘D hands back a light theme, records
    /// it in the other slot, and the pair repairs itself after having done
    /// nothing anybody could see.
    pub fn other_half(&self, dark: bool) -> Option<usize> {
        let wanted = self.text(if dark { "dark_theme" } else { "light_theme" });
        self.themes
            .iter()
            .position(|theme| theme.id == wanted && self.is_dark(theme) == dark)
            .or_else(|| {
                self.themes
                    .iter()
                    .position(|theme| self.is_dark(theme) == dark)
            })
    }

    /// Which theme the machine's own light or dark asks for, if any.
    ///
    /// `followSystemTheme` in `main.ts`. Three ways to answer nothing, and
    /// they are different: the reader has switched following off, the machine
    /// will not say, or the theme in use is already of the right darkness —
    /// the last being the ordinary case and the reason this is cheap to call
    /// on every report.
    pub fn following(&mut self) -> Option<usize> {
        if !self.flag("follow_system_theme") {
            return None;
        }
        let outside = self.outside?;
        // A choice made against the machine holds while the machine says
        // what it said then — across a relaunch, which is not a switch — and
        // lapses once it switches.
        match self.text("theme_chosen_against") {
            against if against.is_empty() => {}
            against if against == darkness(Some(outside)) => return None,
            _ => self.stop_overruling(),
        }
        if self.dark_now() == outside {
            return None;
        }
        self.other_half(outside)
    }

    /// Follow the machine again from now, whatever was chosen against it:
    /// the switch turned on means it at once.
    pub fn stop_overruling(&mut self) {
        self.set(vec![("theme_chosen_against".into(), json!(""))]);
    }

    /// The themes, again, because one of the files changed.
    ///
    /// The whole set arrives rather than a filename — that is what
    /// `themes-changed` carries, and fifteen themes of five colours is
    /// cheaper to send than to ask for. Nothing is written down: nobody chose
    /// a theme here, and an editor saving a file every few seconds must not
    /// be a rewrite of `settings.toml` every few seconds.
    pub fn set_themes(&mut self, themes: Vec<theme::Theme>) {
        // `for_now` is a place in the list, and the list is about to change:
        // it follows its theme, or a file arriving in the folder silently
        // changes what the reader is wearing.
        if let Some(worn) = self.for_now.and_then(|index| self.themes.get(index)) {
            let id = worn.id.clone();
            self.for_now = themes.iter().position(|theme| theme.id == id);
        }
        self.themes = themes;
        self.complaint = self.unreadable();
    }

    /// What to wear instead of a theme whose file has gone.
    ///
    /// In order: the theme remembered for that half of the pair, else the
    /// last shipped theme worn in that half, else anything of the same
    /// darkness, else whatever is left. The point of the order is that
    /// somebody who was reading in a dark theme is not put into a light one
    /// because a file was deleted — and that deleting a copy of Nord goes
    /// back to Nord, since wearing the copy took over the remembered slot.
    pub fn replacement_for(&self, gone: &theme::Theme) -> Option<usize> {
        let dark = self.is_dark(gone);
        let remembered = self.text(if dark { "dark_theme" } else { "light_theme" });
        let last_built_in = self.text(if dark {
            "last_built_in_dark"
        } else {
            "last_built_in_light"
        });
        let left = || {
            self.themes
                .iter()
                .enumerate()
                .filter(|(_, theme)| theme.id != gone.id)
        };
        left()
            .find(|(_, theme)| theme.id == remembered)
            .or_else(|| left().find(|(_, theme)| theme.id == last_built_in))
            .or_else(|| left().find(|(_, theme)| self.is_dark(theme) == dark))
            .or_else(|| left().next())
            .map(|(index, _)| index)
    }

    /// Wear a theme for this run only. See `for_now`.
    pub fn wear_for_now(&mut self, index: usize) {
        self.for_now = Some(index);
        self.complaint = self.unreadable();
    }

    /// Whether a theme is the dark one of the pair, judged by its paper —
    /// by [`palette::Palette::dark`]'s measure, so that ⌘D and the chrome
    /// agree about a mid-grey page.
    fn is_dark(&self, theme: &theme::Theme) -> bool {
        let paper = palette::read_colour(&theme.background).unwrap_or(palette::FALLBACK.background);
        palette::luminance(paper) < 0.35
    }

    fn unreadable(&self) -> Option<String> {
        let theme = self.theme();
        let bad = palette::unreadable(theme);
        if bad.is_empty() {
            return None;
        }
        Some(format!(
            "{} names {} the renderer cannot read — colours are #abc, #abcd, #aabbcc or #aabbccdd",
            theme.name,
            bad.join(" and "),
        ))
    }

    /* ------------------------------------------------------- the library */

    /// Say which document this reader has open, and read back what is already
    /// known about it: what to call it, and where the last run left off.
    ///
    /// `touch` is the app's own, and it does two things at once: it moves the
    /// document to the front of the recently-read list, and it *makes an
    /// entry* if there is none. The second is why this is called on open and
    /// not left until somebody marks a page — `toggle_mark` refuses a
    /// document that is not in the library, which is the right answer to a
    /// stale path and the wrong one to a document that was opened a moment
    /// ago.
    ///
    /// `declared` is what the document calls itself, as written — see
    /// [`crate::render::PageSource::title`]. Whether it is worth using is
    /// [`worth_calling`]'s to say, and it is asked *here* rather than at the
    /// renderer because it is the one place that also has the file name to
    /// weigh it against. The app asks the same question a moment later, in
    /// `adoptDocumentTitle`, because pdf.js cannot answer it until the
    /// document has been parsed; pdfium answers it at open, so the toolbar is
    /// never briefly wrong.
    ///
    /// Answers where the reader was, which is `None` for a document that has
    /// not been read before **and for a reader who has turned remembering
    /// off**. That switch is asked here rather than at the caller for the
    /// reason `bootstrap` gives in the app's `lib.rs`: a position handed over
    /// regardless, with the caller expected to ignore it, is two sides that
    /// eventually disagree about whether there was one.
    pub fn opened(&mut self, path: &str, declared: &str) -> Option<Anchor> {
        self.title = called(path, declared);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_secs() as i64)
            .unwrap_or(0);
        self.file = path.to_string();
        let title = self.title.clone();
        let mut place = None;
        // The last document's, until this one's are read: a library that
        // cannot be written below left them under the new file.
        self.marks.clear();
        self.journal.clear();
        // The place the reader just left the last document at is still with
        // the scribe, and this document's may be too — a return within the
        // settle read the place before. See `close_document`, which waits for
        // the same reason.
        flush();
        match library::touch(&self.dir, path, &title, now) {
            Ok(library) => {
                if let Some(entry) = library.files.iter().find(|entry| entry.path == path) {
                    self.marks = entry.marks.clone();
                    self.journal = entry.highlights.clone();
                    place = Some(Anchor {
                        page: entry.page.max(1) as usize,
                        offset: entry.offset,
                    });
                }
            }
            // A library that cannot be written is a reader who loses their
            // marks and their place at the end of the session, which is worth
            // a line at the bottom of the window and is not worth refusing to
            // open a document over.
            Err(refused) => self.complaint = Some(refused),
        }
        // What is open now is deliberately *not* written here. It is one
        // entry per window and a `Store` is one window's, so a store that
        // wrote the list would write a list of one and take the other windows
        // out of it — which is exactly what happened, and it took a session
        // of three windows down to whichever rendered last. Whoever makes a
        // window records what it shows: `Session::window` in the app, and the
        // harness for a reader that has no window at all.
        if !self.flag("remember_position") {
            return None;
        }
        place.filter(|at| at.page > 1 || at.offset > 0.0)
    }

    /// The document has been put down, and this window is showing none.
    ///
    /// **Nothing is written, and that is the whole of the method.** Every
    /// other transition in this file goes through [`Store::opened`], which
    /// calls `library::touch` — and touching a document is what puts it at the
    /// front of the recently-read list. A close that went through the same
    /// door would write an entry keyed by the empty string and move it to the
    /// top of the shelf, which is a row nobody can open standing where the
    /// last thing read should be.
    ///
    /// What *is* written happens above this, in
    /// [`crate::app::Viewer::close_document`]: the reader's place in the
    /// document being put down, through `remember`, while the store still
    /// points at it.
    pub fn closed(&mut self) {
        self.file.clear();
        self.title.clear();
        self.marks.clear();
        self.journal.clear();
    }

    /// The last few documents read, most recent first, for the start screen
    /// and for the Open menu.
    ///
    /// `prune` is the app's own and is why this is not simply `load().files`:
    /// a document that has been moved or deleted is dropped rather than
    /// offered, because a row that cannot be opened is worse than a shorter
    /// list. The one currently open is left out by the caller rather than
    /// here — the start screen has no document to leave out, and the Open menu
    /// does.
    ///
    /// Read from the file on every call rather than kept in memory. It is a
    /// few kilobytes of TOML, it is asked for when a menu opens or a document
    /// closes rather than per frame, and the alternative is a copy that goes
    /// stale the moment the *other* window reads something — which is the
    /// staleness `session.rs` already documents between two windows and the
    /// one place it would actually show.
    pub fn recents(&self) -> Vec<Recent> {
        // Read again only when the file has moved: the start screen and the
        // Open menu ask on every render, and each read is a parse and a
        // `stat` of every document on the shelf.
        let written = std::fs::metadata(library::path(&self.dir))
            .and_then(|meta| meta.modified())
            .ok();
        if let Some((at, shelf)) = self.recents.borrow().as_ref() {
            if *at == written {
                return shelf.clone();
            }
        }
        let shelf: Vec<Recent> = library::prune(&library::load(&self.dir))
            .files
            .into_iter()
            .filter(|entry| !entry.unlisted)
            .take(library::LIMIT)
            .map(|entry| Recent {
                title: if entry.title.is_empty() {
                    file_name(&entry.path)
                } else {
                    entry.title
                },
                page: entry.page.max(1) as usize,
                label: if entry.label.is_empty() {
                    entry.page.max(1).to_string()
                } else {
                    entry.label
                },
                path: entry.path,
            })
            .collect();
        *self.recents.borrow_mut() = Some((written, shelf.clone()));
        shelf
    }

    /// Take a document off that list.
    ///
    /// The app's own gesture — the × that appears on a row of the start
    /// screen when the pointer is over it. A document with marks or
    /// highlights is only taken off the list, until it is opened again; one
    /// without is forgotten. See [`library::forget`].
    pub fn forget(&self, path: &str) {
        // The row goes from the shelf in hand first, and the file follows —
        // the order everything else here writes in. Without it the start
        // screen would show the row until the scribe had been round.
        if let Some((_, shelf)) = self.recents.borrow_mut().as_mut() {
            shelf.retain(|recent| recent.path != path);
        }
        // Through the scribe like every other library write: this is a whole
        // file read, parsed and written back, and it takes the same lock the
        // scribe holds while it records where the reader is.
        let (dir, path) = (self.dir.clone(), path.to_string());
        later(move || {
            refused(&dir, library::forget(&dir, &path));
        });
    }

    /// What to call this document: its own title where that is worth having,
    /// and the file's name where it is not.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The document was rewritten, and a rewritten document may call itself
    /// something else — a paper whose `\title{}` changed between two runs of
    /// LaTeX is the ordinary case. Answers whether the name moved.
    ///
    /// `retitle` is the app's own and writes only when there is a difference,
    /// which is what makes it safe to ask on every reload.
    pub fn renamed(&mut self, declared: &str) -> bool {
        if self.file.is_empty() {
            return false;
        }
        let name = std::path::Path::new(&self.file)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let now = if worth_calling(declared, &name) {
            declared.trim().to_string()
        } else {
            name
        };
        if now == self.title {
            return false;
        }
        self.title = now;
        let (dir, file, title) = (self.dir.clone(), self.file.clone(), self.title.clone());
        later(move || {
            refused(&dir, library::retitle(&dir, &file, &title));
        });
        true
    }

    /// Write down where the reader is, eventually.
    ///
    /// **Eventually is the whole of the design.** This is called on every
    /// change of the scroll offset, which is every wheel event; what it does
    /// is hand a place to `Scribe`, which keeps one per document and writes
    /// when the scrolling has stopped. Nothing here touches the disk, so the
    /// cost on the thread drawing the window is a channel send.
    ///
    /// `label` is what the page is called — "vii", "2681" — so that Recently
    /// read can say what the toolbar said, without opening the document.
    pub fn remember(&self, at: Anchor, label: String) {
        if self.file.is_empty() || !self.flag("remember_position") {
            return;
        }
        let _ = Scribe::get().jobs.send(Job::Place {
            dir: self.dir.clone(),
            file: self.file.clone(),
            page: at.page as u32,
            offset: at.offset,
            label,
        });
    }

    /// The pages the reader has put a pin in, by page number, in page order.
    pub fn marks(&self) -> &[Mark] {
        &self.marks
    }

    pub fn is_marked(&self, page: usize) -> bool {
        self.marks.iter().any(|mark| mark.page as usize == page)
    }

    /// Put a pin in a page, or take it out again — the same gesture doing the
    /// same thing, which is what makes the feature work without ids. Answers
    /// whether the page is marked now.
    ///
    /// `title` is what the row in the sidebar says. The app names a mark for
    /// the section it falls in, which it reads off the outline it has already
    /// walked; that is the caller's to work out, because the outline belongs
    /// to the document and this belongs to the disk.
    pub fn toggle_mark(&mut self, page: usize, title: &str) -> bool {
        if self.file.is_empty() {
            return false;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_secs() as i64)
            .unwrap_or(0);
        let page = page as u32;
        let marked = match self.marks.iter().position(|mark| mark.page == page) {
            Some(at) => {
                self.marks.remove(at);
                false
            }
            None => {
                self.marks.push(Mark {
                    page,
                    offset: 0.0,
                    title: title.to_string(),
                    at: now,
                });
                self.marks.sort_by_key(|mark| mark.page);
                true
            }
        };
        self.write_marks();
        marked
    }

    /// The marks as held here, written down. Memory is the authority for the
    /// length of a session: the file is read once, at open.
    fn write_marks(&self) {
        let (dir, file, marks) = (self.dir.clone(), self.file.clone(), self.marks.clone());
        later(move || {
            refused(&dir, library::set_marks(&dir, &file, marks));
        });
    }

    /// The same for the journal.
    fn write_journal(&self) {
        let (dir, file, journal) = (self.dir.clone(), self.file.clone(), self.journal.clone());
        later(move || {
            refused(&dir, library::set_highlights(&dir, &file, journal));
        });
    }

    /* --------------------------------------------------- the markup journal */

    /// Markup this reader is keeping *beside* the document rather than in it.
    ///
    /// **The journal is never the authority**, which is the app's rule and the
    /// reason `library.rs` says so at the top of `Highlight`: a mark that is
    /// in the file is read out of the file, and this list holds only what a
    /// file cannot carry — markup on a document that could not be written.
    /// Those are the entries the app holds with `annotation_id: null`, and
    /// they are the only ones this reader ever puts here.
    ///
    /// The shape on disk is the app's exactly — the same `library.toml`, the
    /// same eight numbers a run, in the page's own PDF space counting from the
    /// bottom — because `library.rs` is the app's file mounted here rather
    /// than a copy, and a journal one of them writes is a journal the other
    /// reads.
    pub fn journal(&self) -> &[Highlight] {
        &self.journal
    }

    /// Keep one mark beside the document, because it could not go in.
    /// Answers its id, which is what takes it out again.
    pub fn keep_markup(&mut self, page: usize, quads: &[f64], color: &str, quote: &str) -> String {
        let id = format!(
            "{:x}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_nanos())
                .unwrap_or(0),
            self.journal.len()
        );
        if self.file.is_empty() {
            return id;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_secs() as i64)
            .unwrap_or(0);
        let highlight = Highlight {
            id: id.clone(),
            page: page as u32,
            quads: quads.to_vec(),
            color: color.to_string(),
            opacity: MARKUP_OPACITY,
            style: library::HighlightStyle::Highlight,
            quote: quote.to_string(),
            at: now,
            annotation_id: None,
        };
        self.journal.push(highlight);
        self.journal_rev += 1;
        self.write_journal();
        id
    }

    /// Replace the whole journal with what the file itself says, plus
    /// whatever the file could not carry.
    ///
    /// **The journal is a cache and a recovery log, never an authority** —
    /// `library.rs` says so above `Highlight` and this is what makes it true:
    /// everything that was here is discarded in favour of what was just read
    /// out of the document. What the caller keeps is its own business, and
    /// the only things it ever keeps are the two the file cannot say.
    pub fn set_journal(&mut self, highlights: Vec<Highlight>) {
        if self.file.is_empty() || same_journal(&highlights, &self.journal) {
            return;
        }
        self.journal = highlights;
        self.journal_rev += 1;
        self.write_journal();
    }

    /// Which reading of the journal this is. Moves whenever the journal does,
    /// so that anything built from it can tell whether it has to be rebuilt.
    pub fn journal_rev(&self) -> u64 {
        self.journal_rev
    }

    /// One entry of the journal, as this reader writes them.
    pub fn markup_entry(
        page: usize,
        quads: Vec<f64>,
        color: &str,
        quote: &str,
        annotation: Option<String>,
    ) -> Highlight {
        Highlight {
            // The annotation's place is in the id, or two marks of one colour
            // over quotes of one length would share it — and once a rebuild
            // has lost both, removing one would remove the other.
            id: format!(
                "{page}-{}-{}-{}",
                color,
                quote.len(),
                annotation.as_deref().unwrap_or("")
            ),
            page: page as u32,
            quads,
            color: color.to_string(),
            opacity: MARKUP_OPACITY,
            style: library::HighlightStyle::Highlight,
            quote: quote.to_string(),
            at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_secs() as i64)
                .unwrap_or(0),
            annotation_id: annotation,
        }
    }

    /// Take one out of the journal by the id it was given.
    pub fn drop_markup(&mut self, id: &str) {
        if self.file.is_empty() {
            return;
        }
        self.journal.retain(|h| h.id != id);
        self.journal_rev += 1;
        self.write_journal();
    }

    /// Change settings and write them down.
    ///
    /// A group rather than a key, because settings almost never move alone —
    /// a theme with the slot it fills, a zoom with its fit mode — and one call
    /// per key means two whole-file rewrites per change, each re-reading what
    /// the other has just done. That is `App.set` and `flushSettings` in
    /// `main.ts`, and here it is the signature.
    ///
    /// Anything `set_many` would refuse is dropped here, before it reaches
    /// memory: a caller in this crate passing an unknown key is a bug in this
    /// crate rather than something a reader can act on, and the settings in
    /// the same group that were fine still land.
    pub fn set(&mut self, entries: Vec<(String, Value)>) {
        let known = settings::defaults();
        let entries: Vec<(String, Value)> = entries
            .into_iter()
            .filter(|(key, value)| {
                let fine = known
                    .get(key)
                    .is_some_and(|default| settings::same_shape(default, value));
                debug_assert!(fine, "settings refused: {key} = {value}");
                fine
            })
            .collect();
        for (key, value) in &entries {
            self.table().insert(key.clone(), value.clone());
            // Before the write, so the scribe cannot put its older value
            // down after this one. See [`Job::Forget`].
            let _ = Scribe::get().jobs.send(Job::Forget {
                dir: self.dir.clone(),
                key: key.clone(),
            });
        }
        let dir = self.dir.clone();
        later(move || {
            refused(&dir, settings::set_many(&dir, entries));
        });
    }

    /// The same, for a value that is still moving: held in memory now and
    /// written when it stops. See [`Job::Setting`].
    pub fn set_soon(&mut self, entries: Vec<(String, Value)>) {
        let known = settings::defaults();
        for (key, value) in entries {
            // The same door `set` keeps: what the scribe would refuse on
            // disk is not put into memory either.
            if !known
                .get(&key)
                .is_some_and(|default| settings::same_shape(default, &value))
            {
                debug_assert!(false, "settings refused: {key} = {value}");
                continue;
            }
            self.table().insert(key.clone(), value.clone());
            let _ = Scribe::get().jobs.send(Job::Setting {
                dir: self.dir.clone(),
                key,
                value,
            });
        }
    }

    fn table(&self) -> std::sync::MutexGuard<'_, Settings> {
        self.settings.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn text(&self, key: &str) -> String {
        self.table()
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    }

    pub fn flag(&self, key: &str) -> bool {
        self.table()
            .get(key)
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    pub fn number(&self, key: &str) -> f64 {
        self.table()
            .get(key)
            .and_then(Value::as_f64)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("moonowl-store-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// The reader gets the app's fifteen themes, from the app's own files,
    /// with the Moonowl family first — which is what the `order` in each shipped
    /// file is for and the one thing a directory cannot say.
    #[test]
    fn the_shipped_themes_are_there_and_in_their_stated_order() {
        let dir = scratch("themes");
        let store = Store::at(&dir);
        assert_eq!(store.themes().len(), theme::BUILT_IN.len());
        assert!(store.themes().len() >= 14, "{}", store.themes().len());
        assert_eq!(store.themes()[0].id, theme::DEFAULT_LIGHT);
        assert_eq!(store.themes()[1].id, theme::DEFAULT_DARK);
        assert!(store.themes().iter().all(|theme| theme.built_in));
        assert!(store.complaint.is_none(), "{:?}", store.complaint);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A theme is remembered by id and survives the run that chose it.
    #[test]
    fn the_theme_is_remembered() {
        let dir = scratch("wear");
        let dark = {
            let mut store = Store::at(&dir);
            let dark = store
                .themes()
                .iter()
                .position(|theme| theme.id == theme::DEFAULT_DARK)
                .expect("Moonowl Dark ships");
            store.wear(dark);
            // Read off the file rather than restated here. A colour written
            // twice is a colour that drifts, which is the whole reason the
            // shipped set is a directory and not a list.
            assert_eq!(
                store.palette().background,
                palette::read_colour(&store.theme().background).expect("hex"),
            );
            dark
        };

        let reopened = Store::at(&dir);
        assert_eq!(reopened.theme_index(), dark);
        assert_eq!(reopened.theme().id, theme::DEFAULT_DARK);
        // And the slot it fills was written with it, so that following the
        // system later comes back to this rather than to the shipped default.
        assert_eq!(reopened.text("dark_theme"), theme::DEFAULT_DARK);
        assert_eq!(reopened.text("light_theme"), theme::DEFAULT_LIGHT);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ⌘D moves between the two halves of a pair the reader chose, not
    /// between two defaults. Sepia by day and Tokyo Night by night is the
    /// case the two slots exist for.
    #[test]
    fn dark_mode_comes_back_to_the_pair_the_reader_chose() {
        let dir = scratch("pair");
        let mut store = Store::at(&dir);
        let at = |store: &Store, id: &str| {
            store
                .themes()
                .iter()
                .position(|theme| theme.id == id)
                .expect("shipped")
        };
        let (sepia, night) = (at(&store, "sepia"), at(&store, "tokyo-night"));
        store.wear(sepia);
        store.wear(night);
        assert!(store.dark_now());

        // Back to sepia rather than to Moonowl Light, and forward to Tokyo Night
        // rather than to Moonowl Dark — twice each, because the slot is rewritten
        // on every wear and a rule that only holds once is not a rule.
        assert_eq!(store.other_half(false), Some(sepia));
        store.wear(store.other_half(false).expect("a light theme"));
        assert_eq!(store.theme().id, "sepia");
        assert_eq!(store.other_half(true), Some(night));
        store.wear(store.other_half(true).expect("a dark theme"));
        assert_eq!(store.theme().id, "tokyo-night");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The machine's own light and dark, and the three ways of answering
    /// nothing — which are different from each other and all ordinary.
    #[test]
    fn following_the_machine_answers_only_when_there_is_something_to_do() {
        let dir = scratch("following");
        let mut store = Store::at(&dir);
        assert!(store.flag("follow_system_theme"), "on by default");

        // The machine will not say, which is winit on a platform without the
        // notion. Nothing happens, and in particular nothing happens *to
        // light*: a reader wearing a dark theme keeps it.
        let dark = store
            .themes()
            .iter()
            .position(|theme| theme.id == theme::DEFAULT_DARK)
            .expect("Moonowl Dark ships");
        store.wear(dark);
        assert_eq!(store.following(), None);

        // It says dark, and the reader is already dark.
        store.set_outside(Some(true));
        assert_eq!(store.following(), None);

        // It says light, and there is somewhere to go.
        store.set_outside(Some(false));
        let light = store.following().expect("a light theme to move to");
        assert!(!store.is_dark(&store.themes()[light]));

        // And the switch outranks the machine.
        store.set(vec![("follow_system_theme".into(), json!(false))]);
        assert_eq!(store.following(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Choosing a theme that disagrees with the machine holds until the
    /// machine next switches, and leaves the switch alone: no setting moves
    /// another.
    #[test]
    fn choosing_against_the_machine_holds_until_it_switches() {
        let dir = scratch("overrule");
        let mut store = Store::at(&dir);
        store.set_outside(Some(false));

        // Another light theme says nothing about the machine.
        let sepia = store
            .themes()
            .iter()
            .position(|theme| theme.id == "sepia")
            .expect("shipped");
        let worn = store.wear(sepia);
        assert!(!worn.overruled);

        // A dark one on a light machine holds, and following stays on.
        let dark = store
            .themes()
            .iter()
            .position(|theme| theme.id == theme::DEFAULT_DARK)
            .expect("shipped");
        let worn = store.wear(dark);
        assert!(worn.overruled);
        assert!(store.flag("follow_system_theme"));
        assert!(Store::at(&dir).flag("follow_system_theme"));
        assert_eq!(store.following(), None, "the machine saying light again");

        // The machine going dark agrees, and its next switch is followed.
        store.set_outside(Some(true));
        assert_eq!(store.following(), None);
        store.set_outside(Some(false));
        assert!(
            store.following().is_some(),
            "back to light with the machine"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A relaunch is not the machine switching.** The choice was held in
    /// memory, and every launch on a light machine took a dark theme chosen
    /// by hand straight back to light.
    #[test]
    fn a_choice_against_the_machine_survives_a_relaunch() {
        let dir = scratch("overrule-relaunch");
        {
            let mut store = Store::at(&dir);
            store.set_outside(Some(false));
            let dark = store
                .themes()
                .iter()
                .position(|theme| theme.id == theme::DEFAULT_DARK)
                .expect("shipped");
            assert!(store.wear(dark).overruled);
        }
        flush();
        let mut store = Store::at(&dir);
        store.set_outside(Some(false));
        assert_eq!(store.following(), None, "still dark");
        store.set_outside(Some(true));
        assert_eq!(store.following(), None);
        store.set_outside(Some(false));
        assert!(
            store.following().is_some(),
            "and the next switch is followed"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A machine that will not say cannot overrule anybody either, which is
    /// the half of the rule that is easy to write the wrong way round: a
    /// `None` read as "light" would turn following off for every reader on a
    /// platform that does not report it, the first time they chose a dark
    /// theme.
    #[test]
    fn a_silent_machine_neither_moves_nor_stops_anything() {
        let dir = scratch("silent");
        let mut store = Store::at(&dir);
        let dark = store
            .themes()
            .iter()
            .position(|theme| theme.id == theme::DEFAULT_DARK)
            .expect("shipped");
        let worn = store.wear(dark);
        assert!(!worn.overruled);
        assert!(store.flag("follow_system_theme"));
        assert_eq!(store.following(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The theme in use is a name in a file, and a file can name a theme that
    /// is not there — because it was deleted, or because it was written by
    /// hand. Falling back to the default beats falling back to nothing.
    #[test]
    fn a_theme_that_is_gone_falls_back_rather_than_failing() {
        let dir = scratch("gone");
        let mut store = Store::at(&dir);
        store.set(vec![("theme".into(), json!("no-such-theme"))]);
        let reopened = Store::at(&dir);
        assert_eq!(reopened.theme().id, theme::DEFAULT_LIGHT);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A theme somebody wrote by hand is listed after the shipped ones and is
    /// wearable, which is the whole argument for themes being files.
    #[test]
    fn a_hand_written_theme_can_be_worn() {
        let dir = scratch("hand");
        Store::at(&dir);
        std::fs::write(
            dir.join("themes/Mine.toml"),
            "name = \"Mine\"\ntext = \"#102030\"\nbackground = \"#fefefe\"\n",
        )
        .expect("write a theme");

        let mut store = Store::at(&dir);
        let mine = store
            .themes()
            .iter()
            .position(|theme| theme.name == "Mine")
            .expect("listed");
        assert!(
            mine >= theme::BUILT_IN.len(),
            "listed after the shipped set"
        );
        store.wear(mine);
        assert_eq!(store.palette().text, [0x10, 0x20, 0x30]);
        // It is light, so it filled the light slot.
        assert_eq!(store.text("light_theme"), "Mine");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A copy of a shipped dark theme, deleted, hands back to that theme rather
    /// than to Moonowl Dark.
    #[test]
    fn a_deleted_copy_falls_back_to_the_shipped_theme_worn_before_it() {
        let dir = scratch("copy");
        Store::at(&dir);
        std::fs::write(
            dir.join("themes/Mine.toml"),
            "name = \"Mine\"\ntext = \"#e8e8e8\"\nbackground = \"#101018\"\n",
        )
        .expect("write a theme");
        let mut store = Store::at(&dir);
        let find = |store: &Store, id: &str| {
            store
                .themes()
                .iter()
                .position(|theme| theme.id == id)
                .expect(id)
        };
        store.wear(find(&store, "nord"));
        let mine = find(&store, "Mine");
        store.wear(mine);
        let gone = store.themes()[mine].clone();
        let replacement = store.replacement_for(&gone).expect("a replacement");
        assert_eq!(store.themes()[replacement].id, "nord");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The judgement `worth_calling` makes, in the cases that made it exist.
    ///
    /// Every "no" here is a string a real producer writes into a real file,
    /// which is why the test is a list rather than an argument: the rule is
    /// not derivable, it is observed.
    #[test]
    fn a_name_worth_having_is_told_from_one_that_is_not() {
        for (title, file) in [
            ("The Structure of Scientific Revolutions", "kuhn.pdf"),
            ("Attention Is All You Need", "1706.03762v7.pdf"),
        ] {
            assert!(worth_calling(title, file), "{title:?}");
        }
        for (title, file) in [
            // Filled in by the program that made it, not by anybody.
            ("Microsoft Word - report.doc", "report.pdf"),
            ("untitled", "notes.pdf"),
            ("Untitled", "notes.pdf"),
            ("Document1", "notes.pdf"),
            // A title that is a file name is a file name, whatever file it
            // names.
            ("thesis.tex", "thesis.pdf"),
            ("scan_0001.PDF", "scan_0001.pdf"),
            // The file name over again, with and without its suffix.
            ("kuhn", "kuhn.pdf"),
            ("Kuhn.pdf", "kuhn.pdf"),
            // Too short to be a title, and long enough to be a page.
            ("Abc", "paper.pdf"),
            (&"x".repeat(201), "paper.pdf"),
        ] {
            assert!(!worth_calling(title, file), "{title:?}");
        }
        // The app rejects anything *beginning* with the word, not only the
        // word alone — `/^untitled\b/i` — so "Untitled Letters", which is a
        // real book, falls back to its file name. Carried across as written
        // rather than improved on: "Untitled document" and "Untitled 1" are
        // what producers actually emit, the cost of the rule is a file name
        // instead of a name, and a port that quietly disagrees with the app
        // about a judgement is the drift this experiment exists to avoid.
        assert!(!worth_calling("Untitled Letters", "letters.pdf"));
    }

    /// And one that names a colour the renderer cannot read says so, rather
    /// than silently rendering the fallback.
    #[test]
    fn an_unreadable_theme_is_complained_about() {
        let dir = scratch("unreadable");
        Store::at(&dir);
        std::fs::write(
            dir.join("themes/Wrong.toml"),
            "name = \"Wrong\"\ntext = \"steelblue\"\nbackground = \"#fff\"\n",
        )
        .expect("write a theme");

        let mut store = Store::at(&dir);
        let wrong = store
            .themes()
            .iter()
            .position(|theme| theme.name == "Wrong")
            .expect("listed");
        store.wear(wrong);
        let said = store.complaint.clone().expect("a notice");
        assert!(said.contains("Wrong") && said.contains("text"), "{said}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
