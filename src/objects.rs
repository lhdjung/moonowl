//! The document's own objects, read and written through lopdf, for what
//! pdfium cannot write: an incremental update, a smaller file, and the
//! document's details. A PNG is made into a page here too, because decoding
//! it takes as much memory as lopdf does.
//!
//! pdfium does the editing and saves the whole document. What happens here
//! comes after, on pdfium's bytes, and every function falls back to those
//! bytes when lopdf cannot read them or the result does not check out:
//! lopdf is less forgiving of a broken file than pdfium is.
//!
//! **The work is done in a process of its own.** The memory lopdf uses
//! stays with the process that used it, even once it is freed: a 34MB scan
//! left the process 120MB larger, and handing the memory back to the
//! system (`malloc_zone_pressure_relief`) freed none of it. So the app runs
//! its own executable with [`APART`] as its argument, sends it a [`Work`]
//! and the document, and reads back the result; the memory goes when that
//! process ends. Where that process cannot be started, and under
//! `cargo test`, the work is done here, up to [`LIMIT`].

use lopdf::{Document, Object, ObjectId};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::PathBuf;

/// The largest document lopdf works on inside the app's own process. A
/// larger one is written as pdfium saved it. An incremental update, which
/// is how a signed document is changed, is made whatever the size, since
/// the other way to change the document breaks its signature. See the
/// module's comment for why the size matters.
pub const LIMIT: usize = 10 * 1024 * 1024;

/// The argument that makes the app's executable do one piece of lopdf work
/// and end. See [`serve`].
pub const APART: &str = "--lopdf-work";

/// The executable that does the work, once the app has named it. Until then
/// the work is done in this process. Beside it, when the executable was
/// last changed, as it was at start.
static HELPER: std::sync::OnceLock<(PathBuf, Option<std::time::SystemTime>)> =
    std::sync::OnceLock::new();

/// When `executable` was last changed, if the disk says.
fn changed_at(executable: &std::path::Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(executable)
        .and_then(|file| file.modified())
        .ok()
}

/// Do lopdf's work in a process of its own, by running `executable` with
/// [`APART`]. The app names itself at start; a test names the binary cargo
/// built.
pub fn work_apart(executable: PathBuf) {
    let changed = changed_at(&executable);
    let _ = HELPER.set((executable, changed));
}

/// What is done to a document through lopdf, written out so that another
/// process can do it. Everything here is optional, and is done in this
/// order.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Work {
    /// The pages put in this order, by where each was
    /// (`arrange::put_in_order`). Empty leaves them as they are.
    pub order: Vec<usize>,
    /// These pages (zero-based, in their order after `order`) keep only the
    /// resources their content uses (`arrange::slim_pages`): every page of a
    /// new file, and only the pages put in from another file when the
    /// document itself is written.
    pub slim: Vec<usize>,
    /// Every page keeps the label it had before this edit
    /// (`labels::follow`). The number is how many pages the document had
    /// before it, which an arrangement does not say for itself.
    pub follow: Option<(crate::arrange::Edit, usize)>,
    /// What led to removed pages is taken out (`outline::forget_gone`).
    pub forget: bool,
    /// These headings are written as the outline (`outline::write`).
    pub outline: Vec<crate::outline::Entry>,
    /// These links are pointed at their pages again (`outline::relink`).
    pub relink: Vec<crate::outline::Relink>,
    /// The pages from this one (one-based) on numbered as this label begins
    /// (`labels::number_from`).
    pub number: Option<(usize, String, Option<usize>)>,
    /// The title, author and subject written into `/Info`.
    pub details: Option<Details>,
    /// The form and any certification are taken out of the catalog, for a
    /// copy whose fields are drawn into its pages.
    pub unform: bool,
    /// Objects that are the same are written once (`merge_twins`).
    pub merge: bool,
}

/// What the work found, beside the document it wrote.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Done {
    /// Whether the document carries a digital signature ([`signed`]).
    pub signed: bool,
    /// How many headings were taken out by `forget`.
    pub forgot: usize,
    /// Whether some link of `relink` was not where it should be, and so
    /// was not written.
    pub links_lost: bool,
    /// Whether the pages were put in `order`.
    pub ordered: bool,
    /// Why the pages could not be numbered as asked, if they could not.
    pub refused: Option<String>,
}

impl Work {
    /// Do the work on `document`. Answers whether anything was changed.
    fn apply(&self, document: &mut Document, done: &mut Done) -> bool {
        done.signed = signed(document);
        let mut changed = false;
        // First, since everything after it counts pages in their new order.
        // Nothing after it can run on pages still in the old order, so an
        // order that could not be applied ends the work here, and the
        // caller hands the document back as it was.
        if !self.order.is_empty() {
            done.ordered = crate::arrange::put_in_order(document, &self.order);
            if !done.ordered {
                return false;
            }
            changed = true;
        }
        if !self.slim.is_empty() {
            crate::arrange::slim_pages(document, &self.slim);
            changed |= crate::arrange::own_annotations(document);
        }
        if let Some((edit, had)) = &self.follow {
            changed |= crate::labels::follow(document, &edit.moved(*had));
        }
        if self.forget {
            done.forgot = crate::outline::forget_gone(document);
            changed |= done.forgot > 0;
        }
        changed |= crate::outline::write(document, &self.outline);
        let written = crate::outline::relink(document, &self.relink);
        done.links_lost = written < self.relink.len();
        changed |= written > 0;
        if let Some((page, typed, printed)) = &self.number {
            match crate::labels::number_from(document, page.saturating_sub(1), typed, *printed) {
                Ok(()) => changed = true,
                Err(refused) => done.refused = Some(refused),
            }
        }
        if let Some(details) = &self.details {
            set_details(document, details);
            changed = true;
        }
        if self.unform {
            if let Ok(catalog) = document.catalog_mut() {
                changed |= catalog.remove(b"AcroForm").is_some();
                changed |= catalog.remove(b"Perms").is_some();
            }
        }
        if self.merge {
            changed |= merge_twins(document);
        }
        changed
    }
}

/// Every object that is the same as another, such as a font or a picture,
/// made one object, which everything that named either names. pdfium copies
/// a page onto a sheet with everything it uses, one page at a time, so
/// pages that shared a font each carry a copy of it. Pages, the page tree,
/// the catalog, annotations, optional content groups and the structure
/// tree are never merged, since each of them is one thing in one place.
/// Answers whether any objects were merged.
fn merge_twins(document: &mut Document) -> bool {
    use std::hash::{Hash, Hasher};
    // A dictionary that names another can be merged only once that one has
    // been, so this goes on while it finds any, up to a depth that fonts
    // and pictures do not reach.
    let mut merged = false;
    for _ in 0..8 {
        let mut seen: std::collections::HashMap<u64, Vec<ObjectId>> = Default::default();
        let mut same: std::collections::HashMap<ObjectId, ObjectId> = Default::default();
        let placed = |object: &Object| {
            let kind = match object {
                Object::Dictionary(dictionary) => dictionary.get(b"Type").ok(),
                _ => None,
            };
            matches!(
                kind.and_then(|kind| kind.as_name().ok()),
                Some(
                    b"Page"
                        | b"Pages"
                        | b"Catalog"
                        | b"Annot"
                        | b"OCG"
                        | b"StructElem"
                        | b"StructTreeRoot"
                )
            )
        };
        // The text an object prints as is only a first sort: lopdf prints
        // names and strings with every byte that is not UTF-8 replaced, so
        // two palettes can print alike. Objects are the same only if they
        // compare equal. A stream also records where it was read from,
        // which is not part of what it is.
        fn hash_of(object: &Object) -> u64 {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            match object {
                Object::Stream(stream) => {
                    format!("{:?}", stream.dict).hash(&mut hasher);
                    stream.content.hash(&mut hasher);
                }
                other => format!("{other:?}").hash(&mut hasher),
            }
            hasher.finish()
        }
        fn same_as(a: &Object, b: &Object) -> bool {
            match (a, b) {
                (Object::Stream(a), Object::Stream(b)) => {
                    a.dict == b.dict && a.content == b.content
                }
                (Object::Stream(_), _) | (_, Object::Stream(_)) => false,
                (a, b) => a == b,
            }
        }
        for (id, object) in &document.objects {
            if placed(object) {
                continue;
            }
            let twins = seen.entry(hash_of(object)).or_default();
            match (twins.iter()).find(|twin| same_as(&document.objects[*twin], object)) {
                Some(twin) => {
                    same.insert(*id, *twin);
                }
                None => twins.push(*id),
            }
        }
        if same.is_empty() {
            break;
        }
        merged = true;
        fn rename(object: &mut Object, same: &std::collections::HashMap<ObjectId, ObjectId>) {
            match object {
                Object::Reference(id) => {
                    if let Some(twin) = same.get(id) {
                        *id = *twin;
                    }
                }
                Object::Array(items) => items.iter_mut().for_each(|item| rename(item, same)),
                Object::Dictionary(dictionary) => dictionary
                    .iter_mut()
                    .for_each(|(_, value)| rename(value, same)),
                Object::Stream(stream) => stream
                    .dict
                    .iter_mut()
                    .for_each(|(_, value)| rename(value, same)),
                _ => {}
            }
        }
        for object in document.objects.values_mut() {
            rename(object, &same);
        }
        for (_, value) in document.trailer.iter_mut() {
            rename(value, &same);
        }
        document.objects.retain(|id, _| !same.contains_key(id));
    }
    merged
}

/// What an incremental update adds to a signed document, which decides
/// whether a certification allows it. See [`certification_allows`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Update {
    /// Highlights and drawn signatures, which are annotations.
    Annotations,
    /// A form filled in.
    Fields,
    /// Anything else: pages, page labels, the document's details.
    Other,
}

/// Why [`incremental`] did not make an update.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Unwritten {
    /// The document is certified, and its certification does not allow
    /// the change.
    Certified,
    /// A signature locks a form field the change fills in.
    Locked,
    /// The update could not be made or did not check out.
    Failed,
    /// The other process that makes it took too long or ended partway.
    Stopped,
}

/// One piece of work sent to the other process.
#[derive(Serialize, Deserialize)]
enum Ask {
    /// [`rework`], followed by the document.
    Rework(Box<Work>),
    /// [`incremental`], followed by the original and the written document.
    Incremental(Update),
    /// [`picture`], followed by the PNG. The number is how many pixels it
    /// has, which bounds the document that comes back.
    Picture(u64),
    /// [`save_attachment`]. Nothing follows, since the other process reads
    /// the document from the disk itself.
    Attachment(Attachment),
}

/// A file the document carries, to be written to `to`. See
/// [`save_attachment`].
#[derive(Serialize, Deserialize)]
struct Attachment {
    from: String,
    password: Option<String>,
    index: usize,
    name: String,
    to: String,
}

/// The other process's answer, followed by the document it wrote, if any.
#[derive(Serialize, Deserialize)]
enum Answer {
    Reworked(Done),
    Updated,
    Made,
    Saved,
    Declined,
    Certified,
    Locked,
}

/// Write `bytes` with their length before them.
fn send(to: &mut impl Write, bytes: &[u8]) -> std::io::Result<()> {
    to.write_all(&(bytes.len() as u64).to_le_bytes())?;
    to.write_all(bytes)
}

/// Read what [`send`] wrote, if it is at most `most` bytes long. A length
/// past that is an error rather than an allocation, since a failed
/// allocation ends the whole app.
fn receive(from: &mut impl Read, most: usize) -> std::io::Result<Vec<u8>> {
    let mut length = [0; 8];
    from.read_exact(&mut length)?;
    let length = usize::try_from(u64::from_le_bytes(length))
        .ok()
        .filter(|&length| length <= most)
        .ok_or(std::io::ErrorKind::InvalidData)?;
    let mut bytes = vec![0; length];
    from.read_exact(&mut bytes)?;
    Ok(bytes)
}

/// The longest answer the other process may give, apart from a document.
const ANSWER_MOST: usize = 1 << 20;

/// Run `ask` in the other process, with `documents`. Answers `None` when no
/// process was named or it could not be started, so that the work is done
/// here instead, and `Some(None)` when it started but did not finish.
fn apart(ask: &Ask, documents: &[&[u8]]) -> Option<Option<(Answer, Vec<u8>)>> {
    apart_until(ask, documents, false)
}

/// [`apart`], given up on when the app is ending if `quits`, for work whose
/// document is whole without it.
fn apart_until(ask: &Ask, documents: &[&[u8]], quits: bool) -> Option<Option<(Answer, Vec<u8>)>> {
    let (helper, changed) = HELPER.get()?;
    // An app updated while it runs would start the new executable, which
    // may not read what this one sends, so the work is done here instead.
    if changed_at(helper) != *changed {
        return None;
    }
    // An attachment's document is read from the disk by the other process,
    // not sent to it, and is given the same second for every megabyte.
    let read = match ask {
        Ask::Attachment(asked) => std::fs::metadata(&asked.from).map_or(0, |file| file.len()),
        _ => 0,
    };
    apart_with(
        helper,
        ask,
        documents,
        PATIENCE + std::time::Duration::from_secs(read >> 20),
        quits,
    )
}

/// How long the other process is given before it is ended, beside a second
/// for every megabyte it is sent.
const PATIENCE: std::time::Duration = std::time::Duration::from_secs(30);

/// [`apart_until`], with `helper` as the other process, given `patience`.
fn apart_with(
    helper: &std::path::Path,
    ask: &Ask,
    documents: &[&[u8]],
    patience: std::time::Duration,
    quits: bool,
) -> Option<Option<(Answer, Vec<u8>)>> {
    let mut command = std::process::Command::new(helper);
    command
        .arg(APART)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window for a debug build's helper.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().ok()?;
    let (mut input, mut output) = (child.stdin.take()?, child.stdout.take()?);
    // ponytail: a fixed allowance per megabyte; lopdf's speed on a machine
    // much slower than a laptop would want it measured.
    let size: usize = documents.iter().map(|document| document.len()).sum();
    let deadline = patience + std::time::Duration::from_secs((size >> 20) as u64);
    // The work makes a document smaller, or appends a little to one, so
    // a document twice what was sent is nonsense. A picture's page holds
    // at most its pixels, three bytes each.
    let most = match ask {
        Ask::Picture(pixels) => (*pixels as usize).saturating_mul(3),
        _ => 2 * size,
    } + (16 << 20);
    let (answered, heard) = std::sync::mpsc::channel();
    let answer = std::thread::scope(|scope| {
        // Written beside the reading, since a pipe holds only so much.
        scope.spawn(move || {
            let asked = serde_json::to_vec(ask).unwrap_or_default();
            let _ = send(&mut input, &asked);
            for document in documents {
                let _ = send(&mut input, document);
            }
        });
        scope.spawn(move || {
            let mut read = || {
                let answer = receive(&mut output, ANSWER_MOST).ok()?;
                let answer: Answer = serde_json::from_slice(&answer).ok()?;
                let bytes = match answer {
                    Answer::Reworked(_) | Answer::Updated | Answer::Made => {
                        receive(&mut output, most).ok()?
                    }
                    Answer::Saved | Answer::Declined | Answer::Certified | Answer::Locked => {
                        Vec::new()
                    }
                };
                Some((answer, bytes))
            };
            let _ = answered.send(read());
        });
        // A process that answers nonsense or takes too long is ended, which
        // closes its pipes, so that both threads above end too.
        let until = std::time::Instant::now() + deadline;
        let answer = loop {
            let left = until.saturating_duration_since(std::time::Instant::now());
            match heard.recv_timeout(left.min(std::time::Duration::from_millis(50))) {
                Ok(answer) => break answer,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                    if !left.is_zero()
                        && !(quits
                            && crate::export::GIVE_UP
                                .load(std::sync::atomic::Ordering::Relaxed)) => {}
                Err(_) => break None,
            }
        };
        if answer.is_none() {
            let _ = child.kill();
        }
        answer
    });
    let ended = child.wait().is_ok_and(|status| status.success());
    Some(answer.filter(|_| ended))
}

/// The other process's side: read one [`Ask`] from stdin, do it, and write
/// the [`Answer`] to stdout. The app's `main` calls this when it is run with
/// [`APART`], before anything else.
pub fn serve() -> std::io::Result<()> {
    let (mut input, mut output) = (std::io::stdin().lock(), std::io::stdout().lock());
    let ask: Ask = serde_json::from_slice(&receive(&mut input, usize::MAX)?)?;
    let (answer, bytes) = match ask {
        Ask::Rework(work) => match rework_here(receive(&mut input, usize::MAX)?, &work, usize::MAX)
        {
            Ok((bytes, done)) => (Answer::Reworked(done), bytes),
            Err(_) => (Answer::Declined, Vec::new()),
        },
        Ask::Incremental(update) => {
            let original = receive(&mut input, usize::MAX)?;
            match incremental_here(&original, &receive(&mut input, usize::MAX)?, update) {
                Ok(bytes) => (Answer::Updated, bytes),
                Err(Unwritten::Certified) => (Answer::Certified, Vec::new()),
                Err(Unwritten::Locked) => (Answer::Locked, Vec::new()),
                Err(Unwritten::Failed | Unwritten::Stopped) => (Answer::Declined, Vec::new()),
            }
        }
        Ask::Picture(_) => match crate::arrange::picture_pdf(&receive(&mut input, usize::MAX)?) {
            Some(bytes) => (Answer::Made, bytes),
            None => (Answer::Declined, Vec::new()),
        },
        Ask::Attachment(asked) => match attachment_here(&asked) {
            Some(()) => (Answer::Saved, Vec::new()),
            None => (Answer::Declined, Vec::new()),
        },
    };
    send(&mut output, &serde_json::to_vec(&answer)?)?;
    if matches!(answer, Answer::Reworked(_) | Answer::Updated | Answer::Made) {
        send(&mut output, &bytes)?;
    }
    output.flush()
}

/// A PNG of `pixels` pixels as a PDF of one page
/// ([`crate::arrange::picture_pdf`]). Decoding the picture takes memory the
/// app would keep, so it is done in the other process. Done in this
/// process, a picture is refused as too large when its pixels take more
/// than [`LIMIT`].
pub fn picture(png: &[u8], pixels: u64) -> Result<Vec<u8>, Why> {
    match apart(&Ask::Picture(pixels), &[png]) {
        Some(Some((Answer::Made, bytes))) => Ok(bytes),
        Some(Some(_)) => Err(Why::Unreadable),
        Some(None) => Err(Why::Stopped),
        None if pixels.saturating_mul(4) > LIMIT as u64 => Err(Why::TooLarge),
        None => crate::arrange::picture_pdf(png).ok_or(Why::Unreadable),
    }
}

/// Write the file the document at `from` carries at `index`, named `name`,
/// to `to`, in the other process. pdfium decodes such a file twice under its
/// lock, which stops pages being drawn in every window for that long, and
/// keeps the whole file in the app's memory. The other process reads the
/// document from the disk, so nothing is sent to it.
///
/// Answers `false` when there is no other process or it could not write the
/// file, and then the caller writes it through pdfium.
pub fn save_attachment(
    from: &str,
    password: Option<&str>,
    index: usize,
    name: &str,
    to: &str,
) -> bool {
    let asked = Ask::Attachment(Attachment {
        from: from.into(),
        password: password.map(str::to_string),
        index,
        name: name.into(),
        to: to.into(),
    });
    matches!(apart(&asked, &[]), Some(Some((Answer::Saved, _))))
}

/// [`save_attachment`], done here. The files are the leaves of the
/// `/EmbeddedFiles` name tree in the order pdfium counts them, and the name
/// must be the one pdfium gave, or nothing is written.
fn attachment_here(asked: &Attachment) -> Option<()> {
    let document = match &asked.password {
        Some(password) => Document::load_with_password(&asked.from, password),
        None => Document::load(&asked.from),
    }
    .ok()?;
    let names = document.catalog().ok()?.get(b"Names").ok()?;
    let tree = document.dereference(names).ok()?.1.as_dict().ok()?;
    let mut files = Vec::new();
    let mut seen = Default::default();
    leaves(
        &document,
        tree.get(b"EmbeddedFiles").ok()?,
        &mut files,
        0,
        &mut seen,
    );
    let (_, spec) = files.get(asked.index)?;
    let spec = document.dereference(spec).ok()?.1.as_dict().ok()?;
    // pdfium names a file by its file specification, as here, and not by
    // its key in the tree, which a program may make a number.
    let name = (["UF", "F", "DOS", "Mac", "Unix"].iter())
        .find_map(|key| lopdf::decode_text_string(spec.get(key.as_bytes()).ok()?).ok())?;
    if name != asked.name {
        return None;
    }
    let embedded = spec.get(b"EF").ok()?;
    let embedded = document.dereference(embedded).ok()?.1.as_dict().ok()?;
    // pdfium looks for the stream under these keys, in this order.
    let stream = (["UF", "F", "DOS", "Mac", "Unix"].iter())
        .find_map(|key| embedded.get(key.as_bytes()).ok())?;
    let stream = document.dereference(stream).ok()?.1.as_stream().ok()?;
    let bytes = stream.decompressed_content().ok()?;
    crate::atomic_write(std::path::Path::new(&asked.to), &bytes).ok()
}

/// The `(name, value)` pairs of the name tree at `node`, in order. A tree
/// deeper than 32 levels is taken to loop, and a node is read once however
/// often the tree names it, or a crafted tree that names each node twice
/// would take as long as one twice as deep for every level.
fn leaves<'a>(
    document: &'a Document,
    node: &'a Object,
    out: &mut Vec<(&'a Object, &'a Object)>,
    depth: usize,
    seen: &mut std::collections::HashSet<ObjectId>,
) {
    if let Object::Reference(id) = node {
        if !seen.insert(*id) {
            return;
        }
    }
    let Some(node) = (depth < 32)
        .then(|| document.dereference(node).ok())
        .flatten()
        .and_then(|(_, node)| node.as_dict().ok())
    else {
        return;
    };
    // A node with both is read as pdfium reads it: by its names alone.
    if let Ok(Object::Array(names)) = node.get(b"Names") {
        for [name, value] in names.as_chunks::<2>().0 {
            out.push((name, value));
        }
    } else if let Ok(Object::Array(kids)) = node.get(b"Kids") {
        for kid in kids {
            leaves(document, kid, out, depth + 1, seen);
        }
    }
}

/// The most a stream is inflated to while two versions of it are compared.
/// A stream larger than this is taken to have changed.
const COMPARED: usize = 256 * 1024 * 1024;

/// `written`, which pdfium saved from the document that was `original`, as
/// an incremental update of `original`: the original bytes unchanged, with
/// only the objects that changed appended after them. A signature covers the
/// bytes of the file as it was signed, so this is how a signed document is
/// changed without breaking its signature.
///
/// pdfium's save keeps every object's number, which is what makes this
/// possible: an object is compared with the object of the same number in
/// the original. A stream counts as unchanged when only its compression
/// differs, because pdfium compresses every stream it saves.
///
/// Answers `Unwritten::Certified` when the original is certified and its
/// certification does not allow `update`. Answers `Unwritten::Failed` when
/// lopdf cannot read either version, when the original is encrypted or its
/// cross-reference table had to be rebuilt, when the update, read back, is
/// not the document pdfium wrote. Answers `Unwritten::Stopped` when the
/// other process that does it takes too long or ends partway.
pub fn incremental(original: &[u8], written: &[u8], update: Update) -> Result<Vec<u8>, Unwritten> {
    match apart(&Ask::Incremental(update), &[original, written]) {
        Some(Some((Answer::Updated, bytes))) => Ok(bytes),
        Some(Some((Answer::Certified, _))) => Err(Unwritten::Certified),
        Some(Some((Answer::Locked, _))) => Err(Unwritten::Locked),
        Some(Some(_)) => Err(Unwritten::Failed),
        Some(None) => Err(Unwritten::Stopped),
        None => incremental_here(original, written, update),
    }
}

/// [`incremental`], done in this process.
fn incremental_here(original: &[u8], written: &[u8], update: Update) -> Result<Vec<u8>, Unwritten> {
    let old = Document::load_mem(original).map_err(|_| Unwritten::Failed)?;
    if !certification_allows(&old, update) {
        return Err(Unwritten::Certified);
    }
    let new = (Document::load_mem(written).ok())
        .filter(read_whole)
        .ok_or(Unwritten::Failed)?;
    if update == Update::Fields && fills_a_locked_field(&old, &new) {
        return Err(Unwritten::Locked);
    }
    made_incremental(original, old, new).ok_or(Unwritten::Failed)
}

/// Whether `new` gives a form field another value than `old` gave it where
/// a signature in `old` locks that field (its `/Lock`, which the
/// specification calls FieldMDP). Filling it in would make the signature
/// invalid.
fn fills_a_locked_field(old: &Document, new: &Document) -> bool {
    let (before, after) = (fields_of(old), fields_of(new));
    let mut locked = Vec::new();
    for (_, field) in &before {
        let signed =
            field.get(b"FT").and_then(Object::as_name).ok() == Some(b"Sig") && field.has(b"V");
        let Some(lock) = (field.get(b"Lock").ok())
            .filter(|_| signed)
            .and_then(|lock| old.dereference(lock).ok()?.1.as_dict().ok())
        else {
            continue;
        };
        let named: Vec<String> = (lock.get(b"Fields").and_then(Object::as_array).ok())
            .into_iter()
            .flatten()
            .filter_map(|name| lopdf::decode_text_string(name).ok())
            .collect();
        match lock.get(b"Action").and_then(Object::as_name).ok() {
            Some(b"Include") => locked.push(Locks::These(named)),
            Some(b"Exclude") => locked.push(Locks::AllBut(named)),
            _ => locked.push(Locks::AllBut(Vec::new())),
        }
    }
    fn value<'a>(fields: &[(String, &'a lopdf::Dictionary)], name: &str) -> Option<&'a Object> {
        (fields.iter())
            .find(|(other, _)| other == name)
            .and_then(|(_, field)| field.get(b"V").ok())
    }
    (after.iter()).any(|(name, field)| {
        let changed = match (value(&before, name), field.get(b"V").ok()) {
            (Some(was), Some(is)) => !same(was, is),
            (was, is) => was.is_some() != is.is_some(),
        };
        changed
            && (locked.iter()).any(|lock| match lock {
                Locks::These(names) => names.contains(name),
                Locks::AllBut(names) => !names.contains(name),
            })
    })
}

/// Which fields a signature's `/Lock` locks, by their full names.
enum Locks {
    These(Vec<String>),
    AllBut(Vec<String>),
}

/// Every form field of `document` that has a name, by its full name: the
/// names of its parents and its own, joined by points. A tree deeper than
/// 32 levels is taken to loop.
fn fields_of(document: &Document) -> Vec<(String, &lopdf::Dictionary)> {
    fn walk<'a>(
        document: &'a Document,
        node: &'a Object,
        parent: &str,
        depth: usize,
        out: &mut Vec<(String, &'a lopdf::Dictionary)>,
    ) {
        let Some(field) = (depth < 32)
            .then(|| document.dereference(node).ok())
            .flatten()
            .and_then(|(_, node)| node.as_dict().ok())
        else {
            return;
        };
        let own = field
            .get(b"T")
            .ok()
            .and_then(|t| lopdf::decode_text_string(t).ok());
        let name = match (&own, parent.is_empty()) {
            (Some(own), true) => own.clone(),
            (Some(own), false) => format!("{parent}.{own}"),
            (None, _) => parent.to_string(),
        };
        if own.is_some() {
            out.push((name.clone(), field));
        }
        if let Ok(Object::Array(kids)) = field.get(b"Kids") {
            for kid in kids {
                walk(document, kid, &name, depth + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    let form = (document.catalog().ok())
        .and_then(|catalog| catalog.get(b"AcroForm").ok())
        .and_then(|form| document.dereference(form).ok()?.1.as_dict().ok());
    if let Some(Ok(Object::Array(fields))) = form.map(|form| form.get(b"Fields")) {
        for field in fields {
            walk(document, field, "", 0, &mut out);
        }
    }
    out
}

/// The update itself, once the certification allows it.
fn made_incremental(original: &[u8], old: Document, mut new: Document) -> Option<Vec<u8>> {
    if old.was_encrypted() || old.xref_start == 0 {
        return None;
    }
    generations(&mut new, &old);
    let mut update = lopdf::IncrementalDocument::create_from(original.to_vec(), old);
    let changed: Vec<(ObjectId, Object)> = (new.objects.iter())
        .filter(|(_, object)| !layout(object))
        .filter(|(id, object)| {
            let before = update.get_prev_documents().objects.get(id);
            !before.is_some_and(|before| same(before, object))
        })
        .map(|(id, object)| (*id, object.clone()))
        .collect();
    let max_id = update.get_prev_documents().max_id.max(new.max_id);
    let appended = &mut update.new_document;
    for (id, object) in changed {
        appended.objects.insert(id, object);
    }
    appended.max_id = max_id;
    for key in [b"Root".as_slice(), b"Info"] {
        match new.trailer.get(key) {
            Ok(value) => appended.trailer.set(key, value.clone()),
            Err(_) => {
                appended.trailer.remove(key);
            }
        }
    }
    let mut bytes = Vec::new();
    update.save_to(&mut bytes).ok()?;
    let read = Document::load_mem(&bytes).ok()?;
    if read.xref_recovery.is_some() {
        return None;
    }
    let whole = (new.objects.iter())
        .filter(|(_, object)| !layout(object))
        .all(|(id, object)| read.objects.get(id).is_some_and(|got| same(got, object)));
    whole.then_some(bytes)
}

/// A document written as small as lopdf can write it: objects nothing
/// refers to dropped, streams compressed, and the other objects packed into
/// compressed object streams. pdfium leaves a removed page's objects in the
/// file and writes every object on its own, so a document it has saved
/// comes out smaller every time.
/// When the app is ending, the other process is stopped and the document
/// is answered as it was.
pub fn compact(bytes: Vec<u8>) -> Vec<u8> {
    rework_until(bytes, &Work::default(), true)
        .map_or_else(|refused| refused.bytes, |(bytes, _)| bytes)
}

/// The document in `bytes`, changed by `work` and then compacted. A
/// document the work did not change is written only if that makes it
/// smaller.
///
/// Answers `Err` with `bytes` as they were ([`Refused`]) when lopdf cannot read them,
/// when the document is encrypted, when the result does not read back as
/// the same document, when an unchanged document comes out no smaller, or
/// when the other process that does the work fails. Done in this process,
/// a document over [`LIMIT`] is not read at all.
pub fn rework(bytes: Vec<u8>, work: &Work) -> Result<(Vec<u8>, Done), Refused> {
    rework_until(bytes, work, false)
}

/// [`rework`], given up on when the app is ending if `quits`.
fn rework_until(bytes: Vec<u8>, work: &Work, quits: bool) -> Result<(Vec<u8>, Done), Refused> {
    match apart_until(&Ask::Rework(Box::new(work.clone())), &[&bytes], quits) {
        Some(Some((Answer::Reworked(done), written))) => Ok((written, done)),
        Some(Some(_)) => Err(Refused {
            bytes,
            why: Why::Unreadable,
        }),
        Some(None) => Err(Refused {
            bytes,
            why: Why::Stopped,
        }),
        None => rework_here(bytes, work, LIMIT).map_err(|bytes| Refused {
            why: match bytes.len() > LIMIT {
                true => Why::TooLarge,
                false => Why::Unreadable,
            },
            bytes,
        }),
    }
}

/// A document [`rework`] did not change, as it was given.
#[derive(Debug)]
pub struct Refused {
    pub bytes: Vec<u8>,
    pub why: Why,
}

/// Why [`rework`] did not change a document.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Why {
    /// lopdf cannot read it, or the result did not check out.
    Unreadable,
    /// The work was done in this process, and the document is over
    /// [`LIMIT`].
    TooLarge,
    /// The other process that does the work took too long or ended
    /// partway.
    Stopped,
}

/// [`rework`], done in this process, on a document of at most `limit` bytes.
fn rework_here(bytes: Vec<u8>, work: &Work, limit: usize) -> Result<(Vec<u8>, Done), Vec<u8>> {
    if bytes.len() > limit {
        return Err(bytes);
    }
    match Document::load_mem(&bytes) {
        Ok(mut document) if !document.was_encrypted() && read_whole(&document) => {
            let was = Shape::of(&document);
            let mut done = Done::default();
            let changed = work.apply(&mut document, &mut done);
            if !work.order.is_empty() && !done.ordered {
                return Err(bytes);
            }
            smaller(document, bytes, changed, was).map(|written| (written, done))
        }
        _ => Err(bytes),
    }
}

/// What a document read from a file must still be once lopdf has worked on
/// it: the same number of pages, and no more references to objects that
/// are not there.
struct Shape {
    pages: usize,
    missing: usize,
}

impl Shape {
    fn of(document: &Document) -> Shape {
        let mut missing = 0;
        let mut waiting: Vec<&Object> = document.objects.values().collect();
        waiting.extend(document.trailer.iter().map(|(_, value)| value));
        while let Some(object) = waiting.pop() {
            match object {
                Object::Reference(id) => missing += !document.objects.contains_key(id) as usize,
                Object::Array(items) => waiting.extend(items),
                Object::Dictionary(dictionary) => waiting.extend(dictionary.iter().map(|(_, v)| v)),
                Object::Stream(stream) => waiting.extend(stream.dict.iter().map(|(_, v)| v)),
                _ => {}
            }
        }
        Shape {
            pages: document.get_pages().len(),
            missing,
        }
    }

    /// Whether `self`, after the work, is no worse than `was`, before it.
    fn holds(&self, was: &Shape) -> bool {
        self.pages == was.pages && self.missing <= was.missing
    }
}

/// `document`, which was read from `bytes`, written as [`compact`] writes
/// it. Unless it was `changed`, it is written only if that is smaller. What
/// is written is read back and compared with `document`, and with `was`,
/// the shape of the document before the work.
fn smaller(
    mut document: Document,
    bytes: Vec<u8>,
    changed: bool,
    was: Shape,
) -> Result<Vec<u8>, Vec<u8>> {
    prune(&mut document);
    // PDF/A forbids a filter on the XMP metadata, so that it can be read
    // without a PDF reader, and pdfium leaves it uncompressed too.
    for object in document.objects.values_mut() {
        if let Object::Stream(stream) = object {
            if stream.dict.get(b"Type").and_then(Object::as_name).ok() == Some(b"Metadata") {
                stream.allows_compression = false;
            }
        }
    }
    document.compress();
    // Object and cross-reference streams came with PDF 1.5. A file that says
    // it is older, as PDF/A-1 does, is written without them.
    let modern = document.version.as_str() >= "1.5";
    let options = lopdf::SaveOptions::builder()
        .use_object_streams(modern)
        .use_xref_streams(modern)
        .build();
    let mut written = Vec::new();
    if document.save_with_options(&mut written, options).is_err()
        || (!changed && written.len() >= bytes.len())
    {
        return Err(bytes);
    }
    let Ok(read) = Document::load_mem(&written) else {
        return Err(bytes);
    };
    if read.xref_recovery.is_some() || !Shape::of(&read).holds(&was) {
        return Err(bytes);
    }
    let whole = (document.objects.iter())
        .filter(|(_, object)| !layout(object))
        .all(|(id, object)| read.objects.get(id).is_some_and(|got| same(got, object)));
    if whole {
        Ok(written)
    } else {
        Err(bytes)
    }
}

/// pdfium writes every object as generation 0. An object that `old`
/// numbers with a later generation is given that generation back in `new`,
/// with every reference to it, so that it compares with the original and
/// is not appended again with everything that refers to it.
fn generations(new: &mut Document, old: &Document) {
    let later: std::collections::HashMap<u32, u16> = (old.objects.keys())
        .filter(|&&(number, generation)| generation != 0 && !old.objects.contains_key(&(number, 0)))
        .copied()
        .collect();
    if later.is_empty() {
        return;
    }
    fn renumber(object: &mut Object, later: &std::collections::HashMap<u32, u16>) {
        let entries = match object {
            Object::Reference((number, generation)) => {
                if *generation == 0 {
                    *generation = later.get(number).copied().unwrap_or(0);
                }
                return;
            }
            Object::Array(items) => {
                items.iter_mut().for_each(|item| renumber(item, later));
                return;
            }
            Object::Dictionary(entries) => entries,
            Object::Stream(stream) => &mut stream.dict,
            _ => return,
        };
        entries
            .iter_mut()
            .for_each(|(_, value)| renumber(value, later));
    }
    new.objects = std::mem::take(&mut new.objects)
        .into_iter()
        .map(|((number, generation), mut object)| {
            renumber(&mut object, &later);
            let generation = match generation {
                0 => later.get(&number).copied().unwrap_or(0),
                given => given,
            };
            ((number, generation), object)
        })
        .collect();
    (new.trailer.iter_mut()).for_each(|(_, value)| renumber(value, &later));
}

/// Whether lopdf read every object the file lists. Reading leniently, it
/// skips an object it cannot parse, and a document written from what it
/// read would be without that object.
fn read_whole(document: &Document) -> bool {
    use lopdf::xref::XrefEntry;
    (document.reference_table.entries.iter()).all(|(&number, entry)| match *entry {
        XrefEntry::Normal { generation, .. } => {
            document.objects.contains_key(&(number, generation))
        }
        XrefEntry::Compressed { .. } => document.objects.contains_key(&(number, 0)),
        _ => true,
    })
}

/// Whether a document allows `update` to be added. A document that is not
/// certified allows any update. A certification's `/P` says what may
/// change: 1 nothing, 2 only forms and signatures, 3 annotations too. No
/// certification allows other changes, such as page labels or the
/// document's details. A certification that cannot be read allows nothing.
fn certification_allows(document: &Document, update: Update) -> bool {
    fn dictionary<'a>(document: &'a Document, object: &'a Object) -> Option<&'a lopdf::Dictionary> {
        document.dereference(object).ok()?.1.as_dict().ok()
    }
    let deref = |object| dictionary(document, object);
    let Ok(catalog) = document.catalog() else {
        return false;
    };
    let Ok(perms) = catalog.get(b"Perms") else {
        return true;
    };
    let Some(perms) = deref(perms) else {
        return false;
    };
    let Ok(signature) = perms.get(b"DocMDP") else {
        return true;
    };
    let Some(signature) = deref(signature) else {
        return false;
    };
    let allowed = (signature.get(b"Reference").and_then(Object::as_array).ok())
        .into_iter()
        .flatten()
        .filter_map(deref)
        .filter_map(|reference| deref(reference.get(b"TransformParams").ok()?))
        .find_map(|params| params.get(b"P").and_then(Object::as_i64).ok())
        .unwrap_or(2);
    match update {
        Update::Fields => allowed >= 2,
        Update::Annotations => allowed >= 3,
        Update::Other => false,
    }
}

/// Whether an object is part of how the file is laid out rather than of the
/// document: a stream of objects or of cross-references, which a save makes
/// anew.
fn layout(object: &Object) -> bool {
    matches!(object.type_name(), Ok(b"ObjStm" | b"XRef"))
}

/// Whether two versions of an object say the same thing. A string is the
/// same whether it is written in hexadecimal or not, a number whether it
/// is written with a point or not, and a stream is compared by what it
/// holds, not by how it is compressed. pdfium writes numbers with fewer
/// digits than some files have, so numbers within a hundred-thousandth of
/// their size of each other are the same.
fn same(one: &Object, other: &Object) -> bool {
    let close = |one: f32, other: f32| (one - other).abs() <= 1e-5 * one.abs().max(1.0);
    match (one, other) {
        (Object::String(one, _), Object::String(other, _)) => one == other,
        (Object::Integer(whole), Object::Real(real))
        | (Object::Real(real), Object::Integer(whole)) => close(*whole as f32, *real),
        (Object::Real(one), Object::Real(other)) => close(*one, *other),
        (Object::Array(one), Object::Array(other)) => {
            one.len() == other.len() && one.iter().zip(other).all(|(a, b)| same(a, b))
        }
        (Object::Dictionary(one), Object::Dictionary(other)) => same_entries(one, other, &[]),
        (Object::Stream(one), Object::Stream(other)) => {
            let encoding = [b"Filter".as_slice(), b"DecodeParms", b"Length"];
            // Equal bytes have an equal length, however `/Length` is written.
            (one.content == other.content && same_entries(&one.dict, &other.dict, &[b"Length"]))
                || (same_entries(&one.dict, &other.dict, &encoding)
                    && content_of(one).is_some_and(|content| Some(content) == content_of(other)))
        }
        _ => one == other,
    }
}

/// Whether two dictionaries have the same entries, apart from `ignored`.
fn same_entries(one: &lopdf::Dictionary, other: &lopdf::Dictionary, ignored: &[&[u8]]) -> bool {
    let kept = |dictionary: &lopdf::Dictionary| {
        (dictionary.iter())
            .filter(|(key, _)| !ignored.contains(&key.as_slice()))
            .count()
    };
    kept(one) == kept(other)
        && (one.iter())
            .filter(|(key, _)| !ignored.contains(&key.as_slice()))
            .all(|(key, value)| other.get(key).is_ok_and(|theirs| same(value, theirs)))
}

/// Drop every object that cannot be reached from the trailer. lopdf's own
/// `prune_objects` takes seconds on a long book, because it looks each
/// reference up in a list.
fn prune(document: &mut Document) {
    let mut reached = std::collections::HashSet::new();
    let mut waiting: Vec<&Object> = document.trailer.iter().map(|(_, value)| value).collect();
    while let Some(object) = waiting.pop() {
        match object {
            Object::Reference(id) => {
                if reached.insert(*id) {
                    waiting.extend(document.objects.get(id));
                }
            }
            Object::Array(items) => waiting.extend(items),
            Object::Dictionary(dictionary) => waiting.extend(dictionary.iter().map(|(_, v)| v)),
            Object::Stream(stream) => waiting.extend(stream.dict.iter().map(|(_, v)| v)),
            _ => {}
        }
    }
    document.objects.retain(|id, _| reached.contains(id));
}

/// What a stream holds once its filters are undone, if that can be said.
fn content_of(stream: &lopdf::Stream) -> Option<Vec<u8>> {
    match stream.dict.has(b"Filter") {
        true => stream.decompressed_content_with_limit(COMPARED).ok(),
        false => Some(stream.content.clone()),
    }
}

/// The details of a document a reader can change, as its `/Info` gives
/// them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Details {
    pub title: String,
    pub author: String,
    pub subject: String,
}

/// Said when lopdf does not read a document whose details are to change.
const DETAILS_NOT_WRITTEN: &str = "The details of this document cannot be changed here: \
     its file is written in a way that cannot be changed safely.";

/// Said when a document is too large to be changed in the app's own process.
pub const TOO_LARGE: &str = "it is too large to be changed here.";

/// Said when the other process that changes a document took too long or
/// ended partway.
pub const STOPPED: &str = "changing the file took too long or stopped partway, \
     so the file was left as it was.";

/// Write `details` into the document at `path`, with the time of the change.
/// A detail left empty is taken out. The document's XMP metadata, where it
/// has some, is left as it is.
// ponytail: other programs that prefer XMP to `/Info` show the old title;
// write `dc:title` too if that is ever seen.
pub fn write_details(path: &str, details: &Details) -> Result<(), String> {
    let work = Work {
        details: Some(details.clone()),
        ..Work::default()
    };
    crate::markup::rewrite(path, Update::Other, |bytes| match rework(bytes, &work) {
        Ok((bytes, done)) => Ok(crate::markup::Saved {
            bytes,
            signed: done.signed,
        }),
        Err(refused) => Err(match refused.why {
            Why::TooLarge => {
                format!("The details of this document cannot be changed: {TOO_LARGE}")
            }
            Why::Stopped => {
                format!("The details of this document could not be changed: {STOPPED}")
            }
            Why::Unreadable => DETAILS_NOT_WRITTEN.into(),
        }),
    })
}

/// [`write_details`]'s change to the document.
fn set_details(document: &mut Document, details: &Details) {
    let info = match document.trailer.get(b"Info") {
        Ok(Object::Reference(id)) if document.get_dictionary(*id).is_ok() => *id,
        held => {
            let info = match held {
                Ok(Object::Dictionary(info)) => info.clone(),
                _ => lopdf::Dictionary::new(),
            };
            let id = document.add_object(info);
            document.trailer.set("Info", id);
            id
        }
    };
    let now = chrono::Utc::now().format("D:%Y%m%d%H%M%SZ").to_string();
    let Ok(info) = document.get_dictionary_mut(info) else {
        return;
    };
    let given = [
        ("Title", &details.title),
        ("Author", &details.author),
        ("Subject", &details.subject),
    ];
    for (key, value) in given {
        match value.trim() {
            "" => {
                info.remove(key.as_bytes());
            }
            value => info.set(key, lopdf::text_string(value)),
        }
    }
    info.set("ModDate", Object::string_literal(now));
}

/// Whether the document carries a digital signature: a signature dictionary
/// with something in its `/Contents`. A blank signature field is not one.
pub fn signed(document: &Document) -> bool {
    document.objects.values().any(|object| {
        let dictionary = match object {
            Object::Dictionary(dictionary) => dictionary,
            Object::Stream(stream) => &stream.dict,
            _ => return false,
        };
        dictionary.has(b"ByteRange")
            && (dictionary.get(b"Contents"))
                .and_then(Object::as_str)
                .is_ok_and(|contents| contents.iter().any(|&byte| byte != 0))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{dictionary, Stream};

    /// Two pictures with the same pixels and palettes that differ only in
    /// bytes that are not UTF-8 stay two pictures, and two that are the
    /// same become one.
    #[test]
    fn pictures_that_differ_in_their_palettes_are_not_merged() {
        let mut document = Document::with_version("1.7");
        let picture = |palette: &[u8]| {
            Stream::new(
                dictionary! {
                    "Type" => "XObject", "Subtype" => "Image", "Width" => 2, "Height" => 1,
                    "BitsPerComponent" => 8,
                    "ColorSpace" => vec![
                        "Indexed".into(), "DeviceRGB".into(), 1.into(),
                        Object::String(palette.to_vec(), lopdf::StringFormat::Literal),
                    ],
                },
                vec![0, 1],
            )
        };
        let red = document.add_object(picture(&[0xff, 0x80, 0x80, 0xc0, 0x90, 0x90]));
        let blue = document.add_object(picture(&[0x80, 0x80, 0xff, 0x90, 0x90, 0xc0]));
        let again = document.add_object(picture(&[0xff, 0x80, 0x80, 0xc0, 0x90, 0x90]));
        assert!(merge_twins(&mut document));
        assert!(document.objects.contains_key(&red));
        assert!(document.objects.contains_key(&blue));
        assert!(!document.objects.contains_key(&again));
    }

    /// **A name tree that names each node twice is read in a moment**, not
    /// in the two to the thirtieth steps a walk of every name would take.
    #[test]
    fn a_tree_naming_its_nodes_twice_is_read_once() {
        let mut document = Document::with_version("1.7");
        let mut node = document.add_object(dictionary! {
            "Names" => vec![Object::string_literal("data.csv"), Object::Null],
        });
        for _ in 0..30 {
            node = document.add_object(dictionary! { "Kids" => vec![node.into(), node.into()] });
        }
        let began = std::time::Instant::now();
        let mut out = Vec::new();
        let root = Object::Reference(node);
        leaves(&document, &root, &mut out, 0, &mut Default::default());
        assert_eq!(out.len(), 1);
        assert!(began.elapsed().as_secs() < 1, "{:?}", began.elapsed());
    }

    /// A document of one page, saved with an old-style cross-reference
    /// table or, with `modern`, with object streams and a cross-reference
    /// stream.
    fn one_page(modern: bool) -> Vec<u8> {
        let mut document = Document::with_version("1.7");
        let tree = document.new_object_id();
        let content = document.add_object(Stream::new(dictionary! {}, b"0 0 m".to_vec()));
        let page = document.add_object(dictionary! {
            "Type" => "Page", "Parent" => tree, "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        });
        let pages = dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 };
        document.objects.insert(tree, Object::Dictionary(pages));
        let catalog = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => tree });
        document.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        if modern {
            document.save_modern(&mut bytes).unwrap();
        } else {
            document.reference_table.cross_reference_type =
                lopdf::xref::XrefType::CrossReferenceTable;
            document.save_to(&mut bytes).unwrap();
        }
        bytes
    }

    #[test]
    fn a_result_that_lost_an_object_or_a_page_does_not_hold() {
        let document = Document::load_mem(&one_page(false)).unwrap();
        let was = Shape::of(&document);
        assert!(Shape::of(&document).holds(&was));
        let mut lost = document.clone();
        let page = *lost.get_pages().get(&1).unwrap();
        let content = lost.get_page_contents(page)[0];
        lost.objects.remove(&content);
        assert!(!Shape::of(&lost).holds(&was), "a page's content went");
        let mut fewer = document.clone();
        let tree = fewer
            .catalog()
            .unwrap()
            .get(b"Pages")
            .unwrap()
            .as_reference()
            .unwrap();
        let tree = fewer.get_dictionary_mut(tree).unwrap();
        tree.set("Kids", Vec::<Object>::new());
        tree.set("Count", 0);
        assert!(!Shape::of(&fewer).holds(&was), "a page went");
    }

    #[test]
    fn numbers_written_with_fewer_digits_are_the_same() {
        assert!(same(&Object::Real(1.234_567_9), &Object::Real(1.23457)));
        assert!(same(&Object::Real(612.000_01), &Object::Integer(612)));
        assert!(!same(&Object::Real(1.2345), &Object::Real(1.2346)));
    }

    #[test]
    fn a_pdf_a_file_is_compacted_as_pdf_a_allows() {
        let mut document = Document::with_version("1.4");
        let tree = document.new_object_id();
        let content = b"0 0 m ".repeat(1000);
        let content = document.add_object(Stream::new(dictionary! {}, content));
        let xmp = b"<x:xmpmeta pdfaid:part='1'/> ".repeat(100);
        let xmp = Stream::new(
            dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
            xmp,
        );
        let xmp = document.add_object(xmp);
        let page = document.add_object(dictionary! {
            "Type" => "Page", "Parent" => tree, "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        });
        let pages = dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 };
        document.objects.insert(tree, Object::Dictionary(pages));
        let catalog = dictionary! { "Type" => "Catalog", "Pages" => tree, "Metadata" => xmp };
        let catalog = document.add_object(catalog);
        document.trailer.set("Root", catalog);
        document.reference_table.cross_reference_type = lopdf::xref::XrefType::CrossReferenceTable;
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();

        let compacted = compact(bytes.clone());
        assert!(compacted.len() < bytes.len(), "the content is compressed");
        let read = Document::load_mem(&compacted).unwrap();
        let xmp = read.catalog().unwrap().get(b"Metadata").unwrap();
        let xmp = read.dereference(xmp).unwrap().1.as_stream().unwrap();
        assert!(!xmp.dict.has(b"Filter"), "the metadata is not");
        let streams = compacted.windows(7).filter(|w| w == b"/ObjStm").count();
        assert_eq!(streams, 0, "and no object streams in PDF 1.4");
    }

    /// `bytes` with a title put into the page and the content compressed,
    /// which is what pdfium does to every stream it saves.
    fn changed(bytes: &[u8]) -> Vec<u8> {
        let mut document = Document::load_mem(bytes).unwrap();
        let page = *document.get_pages().get(&1).unwrap();
        let dictionary = document.get_dictionary_mut(page).unwrap();
        dictionary.set("UserUnit", 2);
        document.compress();
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn an_update_keeps_the_original_bytes_and_appends_only_what_changed() {
        for modern in [false, true] {
            let original = one_page(modern);
            let written = changed(&original);
            let update = incremental(&original, &written, Update::Other).expect("an update");
            assert!(update.starts_with(&original), "the original bytes stay");
            let appended = &update[original.len()..];
            let objects = appended.windows(4).filter(|w| w == b" obj").count();
            let streams = appended.windows(5).filter(|w| w == b"/XRef").count();
            assert_eq!(streams, modern as usize, "the original's kind of table");
            assert_eq!(objects - streams, 1, "only the page is appended");
            let read = Document::load_mem(&update).unwrap();
            let page = *read.get_pages().get(&1).unwrap();
            let unit = read.get_dictionary(page).unwrap().get(b"UserUnit").unwrap();
            assert_eq!(unit.as_i64().unwrap(), 2);
        }
    }

    #[test]
    fn a_compacted_document_drops_what_nothing_refers_to() {
        let mut document = Document::load_mem(&one_page(false)).unwrap();
        let orphan = document.add_object(Stream::new(dictionary! {}, vec![b'x'; 4000]));
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        let compacted = compact(bytes.clone());
        assert!(compacted.len() < bytes.len());
        let read = Document::load_mem(&compacted).unwrap();
        assert!(!read.objects.contains_key(&orphan));
        assert_eq!(read.get_pages().len(), 1);
        // What cannot be read is handed back as it was.
        assert_eq!(compact(b"not a pdf".to_vec()), b"not a pdf");
    }

    #[test]
    fn an_object_is_the_same_however_it_is_spelled() {
        assert!(same(&Object::Real(400.0), &Object::Integer(400)));
        assert!(!same(&Object::Real(400.5), &Object::Integer(400)));
        let literal = Object::String(b"a".to_vec(), lopdf::StringFormat::Literal);
        let hex = Object::String(b"a".to_vec(), lopdf::StringFormat::Hexadecimal);
        assert!(same(
            &Object::Array(vec![literal]),
            &Object::Array(vec![hex])
        ));
        let roomy = Stream::new(dictionary! {}, vec![b'0'; 200]);
        let mut squeezed = roomy.clone();
        squeezed.compress().unwrap();
        assert!(squeezed.dict.has(b"Filter"));
        assert!(same(&Object::Stream(roomy), &Object::Stream(squeezed)));
        let other = Stream::new(dictionary! {}, vec![b'1'; 200]);
        let zeros = Stream::new(dictionary! {}, vec![b'0'; 200]);
        assert!(!same(&Object::Stream(zeros), &Object::Stream(other)));
    }

    #[test]
    fn only_a_signature_with_contents_is_a_signature() {
        let mut document = Document::with_version("1.7");
        let blank = dictionary! { "Type" => "Sig", "ByteRange" => vec![0.into()],
        "Contents" => Object::String(vec![0; 8], lopdf::StringFormat::Hexadecimal) };
        let id = document.add_object(blank);
        assert!(!signed(&document));
        let filled = dictionary! { "Type" => "Sig", "ByteRange" => vec![0.into()],
        "Contents" => Object::String(vec![0x30, 0x82], lopdf::StringFormat::Hexadecimal) };
        document.objects.insert(id, Object::Dictionary(filled));
        assert!(signed(&document));
    }

    #[test]
    fn an_object_keeps_its_generation_in_an_update() {
        let made = |generation: u16| {
            let mut document = Document::with_version("1.7");
            let content = (50, generation);
            let stream = Stream::new(dictionary! {}, b"0 0 m".to_vec());
            document.objects.insert(content, Object::Stream(stream));
            let page = document.add_object(dictionary! { "Contents" => content });
            document.trailer.set("Root", page);
            document
        };
        let (old, mut new) = (made(3), made(0));
        generations(&mut new, &old);
        assert!(new.objects.contains_key(&(50, 3)));
        let page = new.trailer.get(b"Root").unwrap().as_reference().unwrap();
        let contents = new.get_dictionary(page).unwrap().get(b"Contents").unwrap();
        assert_eq!(contents.as_reference().unwrap(), (50, 3));
    }

    /// Only a document refused for its size is said to be too large.
    #[test]
    fn a_refusal_says_whether_the_size_was_the_reason() {
        let large = rework(vec![0; LIMIT + 1], &Work::default()).unwrap_err();
        assert!(large.why == Why::TooLarge && large.bytes.len() == LIMIT + 1);
        let unreadable = rework(b"not a document".to_vec(), &Work::default()).unwrap_err();
        assert_eq!(unreadable.why, Why::Unreadable);
    }

    /// Labels, headings and links are all written by page position, so a
    /// document whose pages could not be put in order is handed back whole
    /// rather than worked on in the wrong order.
    #[test]
    fn an_order_that_cannot_be_applied_refuses_the_whole_work() {
        let bytes = one_page(false);
        let work = Work {
            order: vec![1, 0],
            number: Some((1, "iv".into(), None)),
            ..Work::default()
        };
        let refused = rework_here(bytes.clone(), &work, LIMIT).unwrap_err();
        assert_eq!(refused, bytes);
    }

    #[test]
    fn a_document_with_an_object_lopdf_cannot_read_is_not_rewritten() {
        let bytes = one_page(false);
        let at = bytes.windows(9).position(|w| w == b"/MediaBox").unwrap();
        let mut broken = bytes.clone();
        broken.splice(at..at, b"/Broken [[ ".iter().copied());
        assert!(Document::load_mem(&broken).is_ok(), "lopdf reads around it");
        assert!(rework_here(broken, &Work::default(), LIMIT).is_err());
    }

    #[test]
    fn a_certified_document_takes_highlights_only_if_it_allows_them_and_nothing_else() {
        let certified = |allowed: i64, update: Update| {
            let mut document = Document::with_version("1.7");
            let params = dictionary! { "Type" => "TransformParams", "P" => allowed };
            let reference =
                dictionary! { "TransformMethod" => "DocMDP", "TransformParams" => params };
            let signature = document.add_object(dictionary! {
                "Type" => "Sig", "Reference" => vec![reference.into()],
            });
            let catalog = document.add_object(dictionary! {
                "Type" => "Catalog", "Perms" => dictionary! { "DocMDP" => signature },
            });
            document.trailer.set("Root", catalog);
            certification_allows(&document, update)
        };
        let marks = Update::Annotations;
        assert!(!certified(1, marks) && !certified(2, marks) && certified(3, marks));
        assert!(!certified(3, Update::Other), "page labels or details");
        let plain = Document::load_mem(&one_page(false)).unwrap();
        assert!(certification_allows(&plain, Update::Other));
        // A certification lopdf cannot find allows nothing.
        let mut lost = Document::with_version("1.7");
        let missing = lost.new_object_id();
        let catalog = lost.add_object(dictionary! {
            "Type" => "Catalog", "Perms" => dictionary! { "DocMDP" => missing },
        });
        lost.trailer.set("Root", catalog);
        assert!(!certification_allows(&lost, Update::Annotations));
    }

    /// A helper that answers nonsense is ended rather than waited on. This
    /// one sends back what it is sent, so it is still writing the document
    /// into a full pipe when its answer fails to read.
    #[cfg(unix)]
    #[test]
    fn a_helper_that_answers_nonsense_is_ended() {
        use std::os::unix::fs::PermissionsExt;
        let helper = std::env::temp_dir().join(format!("moonowl-echo-{}", std::process::id()));
        std::fs::write(&helper, "#!/bin/sh\nexec cat\n").unwrap();
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
        let document = vec![0; 4 * 1024 * 1024];
        let answer = apart_with(
            &helper,
            &Ask::Incremental(Update::Other),
            &[&document, &document],
            PATIENCE,
            false,
        );
        assert!(matches!(answer, Some(None)));
        let _ = std::fs::remove_file(helper);
    }

    /// A helper that claims an answer longer than memory can hold is not
    /// believed, since allocating that much would end the app.
    #[cfg(unix)]
    #[test]
    fn a_helper_that_claims_a_huge_answer_is_not_believed() {
        use std::os::unix::fs::PermissionsExt;
        let helper = std::env::temp_dir().join(format!("moonowl-huge-{}", std::process::id()));
        let script =
            "#!/bin/sh\ncat > /dev/null\nprintf '\\377\\377\\377\\377\\377\\377\\377\\177'\n";
        std::fs::write(&helper, script).unwrap();
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
        let answer = apart_with(
            &helper,
            &Ask::Incremental(Update::Other),
            &[b"%PDF-"],
            PATIENCE,
            false,
        );
        assert!(matches!(answer, Some(None)));
        let _ = std::fs::remove_file(helper);
    }

    /// A helper that answers well but claims a document far longer than
    /// the one it was sent is not believed either.
    #[cfg(unix)]
    #[test]
    fn a_helper_that_claims_a_huge_document_is_not_believed() {
        use std::os::unix::fs::PermissionsExt;
        let helper = std::env::temp_dir().join(format!("moonowl-long-{}", std::process::id()));
        let answering = |length: &str| {
            let script = format!(
                "#!/bin/sh\ncat > /dev/null\n\
                 printf '\\011\\000\\000\\000\\000\\000\\000\\000\"Updated\"'\n\
                 printf '{length}%%PDF-'\n"
            );
            std::fs::write(&helper, script).unwrap();
            std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
            let asked = Ask::Incremental(Update::Other);
            apart_with(&helper, &asked, &[b"%PDF-"], PATIENCE, false)
        };
        // Five bytes, as long as what was sent, are believed.
        let five = answering("\\005\\000\\000\\000\\000\\000\\000\\000");
        assert!(matches!(five, Some(Some((Answer::Updated, ref bytes))) if bytes == b"%PDF-"));
        // A terabyte is not.
        let huge = answering("\\000\\000\\000\\000\\000\\001\\000\\000");
        assert!(matches!(huge, Some(None)));
        let _ = std::fs::remove_file(helper);
    }

    /// A helper that never answers is ended once its time is up.
    #[cfg(unix)]
    #[test]
    fn a_helper_that_takes_too_long_is_ended() {
        use std::os::unix::fs::PermissionsExt;
        let helper = std::env::temp_dir().join(format!("moonowl-slow-{}", std::process::id()));
        std::fs::write(&helper, "#!/bin/sh\nexec sleep 60\n").unwrap();
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
        let started = std::time::Instant::now();
        let patience = std::time::Duration::from_millis(200);
        let answer = apart_with(
            &helper,
            &Ask::Incremental(Update::Other),
            &[b"%PDF-"],
            patience,
            false,
        );
        assert!(matches!(answer, Some(None)));
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "it was not waited on"
        );
        let _ = std::fs::remove_file(helper);
    }
}
