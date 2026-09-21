//! The reader, run.
//!
//! ```text
//! cargo run --release                          # what you were reading last
//! cargo run --release -- ~/paper.pdf           # a document of your own
//! ```
//!
//! With no path it opens whatever was open when the reader was last put down,
//! and **the start screen** when there was nothing. `reopen_last_document =
//! false` in `settings.toml` turns the restoring off, which is the app's own
//! setting. A path that is not there is said so plainly rather than being
//! handed to pdfium, which reports it as a Debug-printed `io::Error`.

use std::rc::Rc;
use std::sync::Arc;

use moonowl::app::Config;
use moonowl::emit::{Exchange, News, Payload};
use moonowl::session::Session;
use moonowl::shell::Shell;
use moonowl::windows::Desk;
use moonowl::{render, store, watch};

fn main() {
    // Before a document exists, which is what this has to be. See its own
    // comment, and `body` in `styles.rs` for what it buys.
    moonowl::styles::use_variable_fonts();
    let args: Vec<String> = std::env::args().collect();
    // **The window's size is the app's setting, not a number in this file.**
    // It was 1100×900 and never remembered, and that is most of what a reader
    // comparing the two saw as "everything is too small": the app opens at
    // 1280×860 *maximized* (`settings.rs`), so its toolbar has room for the
    // document's name and this one squeezed the name to three letters.
    let remembered = moonowl::settings::load(&moonowl::config::config_dir());
    let setting = |key: &str, fallback: f64| -> f64 {
        remembered
            .get(key)
            .and_then(|value| value.as_f64())
            .unwrap_or(fallback)
    };
    let window_width = setting("window_width", 1280.0);
    let window_height = setting("window_height", 860.0);
    let window_maximized = remembered
        .get("window_maximized")
        .and_then(|value| value.as_bool())
        .unwrap_or(true);
    // `--theme N` is a place in the theme list, and the list is fifteen long
    // rather than two now: it is read out of the app's own `themes/` files,
    // through the app's own loader. Absent means whatever the last run wore.
    let theme = args
        .iter()
        .position(|arg| arg == "--theme")
        .and_then(|at| args.get(at + 1))
        .and_then(|value| value.parse::<usize>().ok());
    let named = args
        .iter()
        .skip(1)
        .find(|arg| moonowl::shell::is_document(std::path::Path::new(arg)))
        .map(|arg| moonowl::config::absolute(arg));
    let config = Config {
        theme,
        ..Config::here()
    };
    // A document named on the command line, else what was open last.
    // `reopening` is where the pruning and the setting are asked — see
    // `store::reopening`: a document that has been moved or deleted would
    // otherwise be reopened, and fail, on every launch for ever.
    // One reader at a time, and a second launch hands its document to the one
    // that is running rather than becoming a second one. See `single.rs`.
    let door = moonowl::single::claim(&config.dir, named.as_deref());
    if matches!(door, moonowl::single::Claim::Second) {
        // Quietly and successfully: the document is on its way to a window
        // that already exists, which is what was asked for.
        return;
    }

    // A document named on the command line is opened here, once, and handed to
    // the launch window below: the line it prints comes out *before* the event
    // loop exists, which is what lets the packaging job open a document on a
    // runner with no display and read "reader: 5 pages" off the log — the one
    // check that says the installed binary found its pdfium. See `bundle.yml`.
    let opened = named.as_deref().map(render::open);
    match (&named, &opened) {
        (Some(path), Some(Ok(document))) => {
            println!("reader: {} pages in {path}", document.pages())
        }
        (Some(path), Some(Err(render::Refusal::Locked))) => {
            println!("reader: {path} is locked — the window will ask for the password")
        }
        (Some(path), Some(Err(err))) => {
            eprintln!("{err}");
            if !std::path::Path::new(path).exists() {
                eprintln!("Run it with no path at all to open whatever you were reading last.");
            }
            std::process::exit(1);
        }
        _ => {}
    }

    // Where the launch window's size waits until the app goes. See the
    // `on_resized` hook below for why it is held rather than written.
    let geometry: Arc<std::sync::Mutex<Option<(f64, f64, bool)>>> =
        Arc::new(std::sync::Mutex::new(None));

    let event_loop = blitz_shell::create_default_event_loop();
    let (proxy, queue) = blitz_shell::BlitzShellProxy::new(event_loop.create_proxy());
    let mut shell = Shell::new(proxy, queue);
    // What the shell says about the windows it makes and closes. Off, because
    // it is a line per window on a run nobody asked to debug — and on with
    // `MOONOWL_TRACE=1`, which is how "did the second window actually land
    // where it was told to" is answered from a terminal rather than with a
    // ruler on the screen.
    shell.trace = std::env::var_os("MOONOWL_TRACE").is_some();
    let windows = shell.windows();

    // What the process holds and every window shares: who is showing what,
    // where news goes, and one watcher over the themes directory and every
    // open document. See `session.rs`.
    let desk = Desk::new();
    let exchange = Exchange::new();
    let watching = Arc::new(watch::start(
        exchange.clone(),
        moonowl::config::themes_dir(),
    ));
    let session_maker = Rc::new(Session {
        desk: desk.clone(),
        exchange: exchange.clone(),
        watching: watching.clone(),
        dir: config.dir.clone(),
        theme: config.theme,
        size: (window_width, window_height),
        maximized: window_maximized,
        remote: windows.remote(),
    });

    // **The launch window, and there is exactly one.** Decided at the first
    // `can_create_surfaces` rather than here, because that is when a Finder
    // launch's documents are known (see `openfiles::launched`): deciding
    // earlier restored the last document and then gave the double-clicked one
    // a second window in front of it. What it is on, in order: the command
    // line, the Finder, the document read most recently (`store::reopening` —
    // restoring every window cascaded them off the screen), the start screen.
    {
        let session = session_maker.clone();
        let dir = config.dir.clone();
        #[cfg(target_os = "macos")]
        let remote = windows.remote();
        shell.on_launch(move || {
            // Asked whatever else is on the table: until it is, every later
            // Finder document is held rather than handed to the shell.
            #[cfg(target_os = "macos")]
            let mut early = moonowl::openfiles::launched().into_iter();
            #[cfg(not(target_os = "macos"))]
            let mut early = std::iter::empty::<String>();
            let first = match (named, opened) {
                (Some(path), Some(opened)) => Some(session.window_over(&path, opened)),
                _ => early
                    .next()
                    .map(|path| session.window(&moonowl::config::absolute(&path))),
            };
            // A multiple selection in the Finder: the rest go where any
            // document from outside goes — tabs or windows, as the reader set.
            #[cfg(target_os = "macos")]
            for rest in early {
                remote.request(Some(rest));
            }
            first
                .unwrap_or_else(|| store::reopening(&dir).and_then(|path| session.window(&path)))
                .or_else(|| session.empty_window())
        });
    }

    // Where a window comes from when one is asked for by path alone: the Dock
    // menu, a second launch, ⌘N. See `Session::hand_over`.
    {
        let session = session_maker.clone();
        shell.on_request(move |path| match path {
            Some(path) => session.hand_over(&moonowl::config::absolute(&path)),
            None => session.another(),
        });
    }
    {
        let session = session_maker.clone();
        shell.on_close(move |label| session.tidy(label));
    }
    {
        // ⌘O: the window kept its identity and changed what is in it, which
        // is the one thing no other path does — every other document arriving
        // in this app arrives with a window of its own.
        let session = session_maker.clone();
        shell.on_swap(move |label, path| session.showing(label, path));
    }
    {
        let desk = desk.clone();
        shell.on_focus(move |label| desk.focused(label.as_deref()));
    }
    {
        // The window changed size, and the document's layout is the one thing
        // in it that will not hear about that on its own — see
        // `Shell::on_resized`. It goes down the mailbox rather than into the
        // window because a component is the only thing that can read the
        // signal, and news is how a component is reached.
        let exchange = exchange.clone();
        let geometry = geometry.clone();
        shell.on_resized(move |label, width, height, maximized| {
            exchange.post(News {
                event: "window-resized".into(),
                target: Some(label.to_string()),
                payload: Payload::Nothing,
            });
            // **Geometry belongs to the launch window**, which is the app's
            // own rule and the app's own reason: there is one remembered size
            // and there are several windows, and letting whichever moved last
            // own it makes the number creep, because what it reads back are
            // windows that were themselves cascaded off it. Held rather than
            // written — a drag is a hundred of these, and each write is a
            // whole file — and put down once, on the way out.
            if label == "main" {
                *geometry.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some((width, height, maximized));
            }
        });
    }
    {
        // Two fingers on the trackpad, which macOS reports as a gesture rather
        // than as a modified wheel — so an application that listens only for
        // ⌃-wheel does not zoom at all. See `Shell::on_pinch`.
        let exchange = exchange.clone();
        shell.on_pinch(move |label, delta| {
            exchange.post(match delta {
                Some(delta) => News {
                    event: "pinched".into(),
                    target: Some(label.to_string()),
                    payload: Payload::Amount(delta),
                },
                None => News {
                    event: "pinch-ended".into(),
                    target: Some(label.to_string()),
                    payload: Payload::Nothing,
                },
            });
        });
    }
    {
        // The machine went light or dark. Same shape as the resize above and
        // for the same reason — the event says only that there is a new
        // answer, and the reader asks the window for it. See
        // `Shell::on_theme`.
        let exchange = exchange.clone();
        shell.on_theme(move |label| {
            exchange.post(News {
                event: "appearance-changed".into(),
                target: Some(label.to_string()),
                payload: Payload::Nothing,
            });
        });
    }
    {
        // A document dragged over a window and let go on it. Everything about
        // it is the window's to answer — the hint it shows, and opening the
        // file through the same `open_here` that ⌘O uses — so all this does is
        // carry winit's word down the mailbox, which is the shape of every
        // other line in this block.
        let exchange = exchange.clone();
        shell.on_drop(move |label, drag| {
            let (event, payload) = match drag {
                moonowl::shell::Drag::Over(t) => ("drag-over", Payload::Takeable(t)),
                moonowl::shell::Drag::Left => ("drag-left", Payload::Nothing),
                moonowl::shell::Drag::Refused => ("drag-refused", Payload::Nothing),
                moonowl::shell::Drag::Drop(path) => ("open-document", Payload::Text(path)),
            };
            exchange.post(News {
                event: event.into(),
                target: Some(label.to_string()),
                payload,
            });
        });
    }
    {
        // Raised before the first window of a quit goes, which is the whole
        // of what tells a window closed by the reader from a window closed
        // because the app is going. See `windows::Desk::closing`.
        let desk = desk.clone();
        shell.on_quit(move || desk.leaving());
    }

    // The door, answered for as long as the process lives, and the Dock's own
    // "New Window" beside it — the one route to a second window that does not
    // need this reader to be in front already, which is exactly the moment
    // somebody wants one.
    #[cfg(unix)]
    if let moonowl::single::Claim::First(listener) = door {
        moonowl::single::serve(listener, windows.remote());
    }
    #[cfg(target_os = "macos")]
    moonowl::dock::install(windows.remote());
    // **What is written on the way out**, as one thing, because there are two
    // ways out: the event loop returning, and a log-out, which AppKit ends the
    // process from inside of. See `openfiles::should_terminate`.
    let farewell = {
        let (geometry, dir) = (geometry.clone(), config.dir.clone());
        move || {
            // How big the window was when the reader put it down, which is how big it
            // comes back. Written here rather than as it changes, for the reason above.
            if let Some((width, height, maximized)) =
                *geometry.lock().unwrap_or_else(|e| e.into_inner())
            {
                let _ = moonowl::settings::set_many(
                    &dir,
                    vec![
                        ("window_width".into(), serde_json::json!(width)),
                        ("window_height".into(), serde_json::json!(height)),
                        ("window_maximized".into(), serde_json::json!(maximized)),
                    ],
                );
            }
            // The socket goes with the process it stood for.
            moonowl::single::release(&dir);
            // Where the reader got to, if the scribe is still holding it. Everything
            // else this reader remembers is written as it changes; a position is
            // written when the scrolling stops, and quitting is the one way to stop
            // scrolling that does not wait. See `store::flush`.
            store::flush();
            // And a highlight still on its way into a document, which a thread is
            // writing and a process that ends takes with it. See `Viewer::write`.
            while moonowl::stats::WRITING.load(std::sync::atomic::Ordering::SeqCst) > 0 {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    };
    // The Finder's own door: a double-clicked document is an Apple Event and
    // not an argument, and it has to be answered before the application
    // finishes launching or the first one is lost. See `openfiles.rs`.
    #[cfg(target_os = "macos")]
    moonowl::openfiles::install(windows.remote(), farewell.clone());

    event_loop.run_app(shell).unwrap();
    farewell();
}
