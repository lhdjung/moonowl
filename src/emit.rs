//! Where news waits for the window it is about.
//!
//! The watcher is a thread and each window's reader is a Dioxus task, so what
//! is needed between them is not a channel but a way for a thread to say
//! "poll me" to a task it cannot see. [`Post`] is one window's mailbox with a
//! waker in it, [`Exchange`] is every window's by name, and [`News`] is what
//! travels: an event, a payload, and either a target or everybody.
//!
//! **This was a shim around Tauri's `AppHandle` and `Emitter`**, so that the
//! app's own `watch.rs` could be mounted here with its `use tauri::…` line
//! untouched — `extern crate self as tauri` and all. Two `use` lines and two
//! call sites were what that bought, against a trait, an `EventTarget`, an
//! error type nothing read, and a `Serialize` bound that turned every payload
//! into JSON in a build with no bridge to send JSON over.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

/// What a window is told, and by whom.
///
/// An event has a name, a payload, and either a target or everybody. The
/// target is the whole difference between one window and several: a
/// recompiled paper reaches the window reading it, and a saved theme reaches
/// all of them.
#[derive(Clone, Debug, PartialEq)]
pub struct News {
    pub event: String,
    pub target: Option<String>,
    pub payload: Payload,
}

/// What comes with an event, which is one of six things.
///
/// It was a `serde_json::Value`, because the shim this module used to be had
/// to satisfy Tauri's `S: Serialize` — the bridge's serialisation surviving
/// in a build with no bridge. Nothing here crosses a process boundary, so
/// these are the shapes themselves.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Payload {
    /// The event is the whole of the message: a resize, an appearance change,
    /// a drag that left.
    #[default]
    Nothing,
    /// A path, or a sentence the notice line is holding.
    Text(String),
    /// What a timer was armed for, so that a stale one can be ignored.
    Token(u64),
    /// How far a pinch moved, as a fraction.
    Amount(f64),
    /// Whether the window is in full screen, with the news that it changed
    /// size — the green button asks nobody.
    Full(bool),
    /// Whether a document over the window is one this reader would open.
    Takeable(bool),
    /// The themes as they now stand — the whole set, which is cheaper to send
    /// than to ask for.
    Themes(Vec<crate::theme::Theme>),
}

/// Where news waits until somebody reads it.
///
/// **A mailbox with a waker in it, and the waker is the load-bearing half.**
/// The watcher is a thread and the reader is a Dioxus task, so what is needed
/// between them is not a channel but a way for the thread to say "poll me" to a
/// task it cannot see. The chain is already built: waking the task marks it
/// ready, which wakes the virtual DOM, which puts an event on the winit loop,
/// which polls the document. Nothing anywhere polls a clock.
///
/// In the harness the same wake makes the next `pump()` run it, which is why a
/// test can drive this with no window and no thread.
#[derive(Clone, Default)]
pub struct Post(Arc<Mutex<Mailbox>>);

#[derive(Default)]
struct Mailbox {
    waiting: VecDeque<News>,
    waker: Option<Waker>,
}

impl Post {
    pub fn new() -> Post {
        Post::default()
    }

    /// Leave news, and wake whoever is waiting for it.
    pub fn send(&self, news: News) {
        let waker = {
            let mut held = self.0.lock().unwrap_or_else(|e| e.into_inner());
            held.waiting.push_back(news);
            held.waker.take()
        };
        // Outside the lock: a waker may run the task inline, and a task that
        // reads the mailbox while the lock is held is a deadlock.
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    /// The next piece of news, whenever it comes.
    pub fn next(&self) -> Next {
        Next(self.clone())
    }

    /// Whatever is waiting, without waiting. What a test reads when it would
    /// rather assert on the news than on what the reader did with it.
    pub fn take(&self) -> Option<News> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .waiting
            .pop_front()
    }
}

pub struct Next(Post);

impl std::future::Future for Next {
    type Output = News;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<News> {
        let mut held = self.0 .0.lock().unwrap_or_else(|e| e.into_inner());
        match held.waiting.pop_front() {
            Some(news) => Poll::Ready(news),
            None => {
                // Replaced rather than kept: a task can be polled by a
                // different waker than the one it registered last time, and
                // waking the stale one wakes nothing.
                held.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

/// Every window's mailbox, by the name the window is known to `watch.rs` by.
///
/// One process watches one themes directory and any number of documents, so
/// there is one watcher and it has to reach a particular window: `watch.rs`
/// reports a rewritten document to the window reading it and the themes to
/// everybody. That is what makes this a switchboard rather than a mailbox.
///
/// A window joins when it is made and leaves when it is destroyed. Leaving
/// matters: news for a window that has gone would otherwise pile up in a
/// mailbox nobody is reading, and the window's `Post` holds a `Waker` into a
/// virtual DOM that no longer exists.
#[derive(Clone, Default)]
pub struct Exchange(Arc<Mutex<BTreeMap<String, Post>>>);

impl Exchange {
    pub fn new() -> Exchange {
        Exchange::default()
    }

    /// A window, and the mailbox it reads.
    pub fn join(&self, label: &str, post: Post) {
        let mut held = self.0.lock().unwrap_or_else(|e| e.into_inner());
        held.insert(label.to_string(), post);
    }

    pub fn leave(&self, label: &str) {
        let mut held = self.0.lock().unwrap_or_else(|e| e.into_inner());
        held.remove(label);
    }

    /// Deliver: to the window named, or to every window when none is.
    pub fn post(&self, news: News) {
        let boxes: Vec<Post> = {
            let held = self.0.lock().unwrap_or_else(|e| e.into_inner());
            match news.target.as_deref() {
                Some(target) => held.get(target).cloned().into_iter().collect(),
                None => held.values().cloned().collect(),
            }
        };
        // Outside the lock, for the reason `Post::send` is: a waker can run a
        // task inline, and that task may be joining or leaving.
        for post in boxes {
            post.send(news.clone());
        }
    }
}

/* ----------------------------------------------------------------- later */

/// News, sent after a delay, on **one** thread for the whole process.
///
/// **This was a thread apiece and that is what a reader's session ran out
/// of.** Two things in this reader say "and put it away again in a moment" —
/// the notice line, and the page pill while somebody is scrolling — and both
/// were `thread::spawn` followed by `sleep`. The pill's is armed by every
/// change in the scroll offset, which is one per frame of a gesture, so a
/// minute of reading is a few thousand threads that exist only to sleep. On
/// macOS the process runs out and `spawn` returns `EAGAIN`, which `std` reports
/// by panicking: `failed to spawn thread: Resource temporarily unavailable`,
/// from a stack with nothing of this app in it.
///
/// One thread, a heap of deadlines and a condvar. It is started on first use
/// and never stopped, like the library's scribe: asleep except when something
/// is due. Arming the same thing again is cheap, which is what lets the pill
/// keep its "restarted by every scroll" shape — the stale ones still fire, and
/// both `unflash_pill` and the notice already ignore an answer that is no
/// longer about anything.
pub fn after(delay: std::time::Duration, post: Post, news: News) {
    use std::sync::Condvar;
    use std::time::Instant;

    struct Clock {
        due: Mutex<Vec<(Instant, Post, News)>>,
        ring: Condvar,
    }

    static CLOCK: std::sync::OnceLock<Arc<Clock>> = std::sync::OnceLock::new();
    let clock = CLOCK.get_or_init(|| {
        let clock = Arc::new(Clock {
            due: Mutex::new(Vec::new()),
            ring: Condvar::new(),
        });
        let ticking = clock.clone();
        let started = std::thread::Builder::new()
            .name("moonowl-clock".into())
            .spawn(move || loop {
                let mut due = ticking.due.lock().unwrap_or_else(|e| e.into_inner());
                // Nothing waiting: sleep until something is left here.
                while due.is_empty() {
                    due = ticking.ring.wait(due).unwrap_or_else(|e| e.into_inner());
                }
                let soonest = due
                    .iter()
                    .map(|(at, _, _)| *at)
                    .min()
                    .unwrap_or_else(Instant::now);
                let now = Instant::now();
                if soonest > now {
                    let (waited, _) = ticking
                        .ring
                        .wait_timeout(due, soonest - now)
                        .unwrap_or_else(|e| e.into_inner());
                    due = waited;
                }
                let now = Instant::now();
                let mut ready = Vec::new();
                due.retain(|(at, post, news)| {
                    if *at <= now {
                        ready.push((post.clone(), news.clone()));
                        false
                    } else {
                        true
                    }
                });
                // Outside the lock: sending wakes a task, and a task that
                // arms another timer would deadlock against it.
                drop(due);
                for (post, news) in ready {
                    post.send(news);
                }
            });
        // A machine that will not give this app one thread is a machine it
        // cannot run on, and there is nothing useful to do about it here —
        // but it is still not worth dying for, so the timer simply never
        // fires and the notice stays up.
        if started.is_err() {
            eprintln!("no thread to keep time on; notices will not clear themselves");
        }
        clock
    });

    let mut due = clock.due.lock().unwrap_or_else(|e| e.into_inner());
    due.push((std::time::Instant::now() + delay, post, news));
    drop(due);
    clock.ring.notify_one();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn news(event: &str, target: Option<&str>) -> News {
        News {
            event: event.to_string(),
            target: target.map(str::to_string),
            payload: Payload::Nothing,
        }
    }

    /// What `emit_to` is for, and the whole of what changed when there was
    /// more than one window: a recompiled paper reaches the window reading it.
    #[test]
    fn news_for_one_window_reaches_that_window_alone() {
        let exchange = Exchange::new();
        let (main, other) = (Post::new(), Post::new());
        exchange.join("main", main.clone());
        exchange.join("reader-1", other.clone());

        exchange.post(news("document-changed", Some("reader-1")));
        assert_eq!(main.take(), None);
        assert_eq!(
            other.take(),
            Some(news("document-changed", Some("reader-1")))
        );
    }

    /// And `emit` with no target is a theme somebody saved, which every window
    /// is wearing.
    #[test]
    fn news_for_nobody_in_particular_reaches_everybody() {
        let exchange = Exchange::new();
        let (main, other) = (Post::new(), Post::new());
        exchange.join("main", main.clone());
        exchange.join("reader-1", other.clone());

        exchange.post(news("themes-changed", None));
        assert!(main.take().is_some());
        assert!(other.take().is_some());
    }

    /// A window that has gone hears nothing. Its mailbox holds a `Waker` into
    /// a virtual DOM that no longer exists, and news nobody reads is a queue
    /// that only grows.
    #[test]
    fn a_window_that_left_hears_nothing() {
        let exchange = Exchange::new();
        let post = Post::new();
        exchange.join("reader-1", post.clone());
        exchange.leave("reader-1");
        exchange.post(news("themes-changed", None));
        assert_eq!(post.take(), None);
    }

    /// Nothing is delivered twice to a window that reported in twice, which is
    /// what a window remade under the same name would be.
    #[test]
    fn a_name_belongs_to_one_mailbox() {
        let exchange = Exchange::new();
        let (first, second) = (Post::new(), Post::new());
        exchange.join("main", first.clone());
        exchange.join("main", second.clone());
        exchange.post(news("themes-changed", None));
        assert_eq!(first.take(), None);
        assert!(second.take().is_some());
    }
}
