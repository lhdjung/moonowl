# Is Dioxus Native a good fit for Moonowl?

Written 2026-08-31 against `dioxus-experiment`, in answer to QRnew's own
assessment saying its migration "is a much better fit for QRnew than it is for
the app the reference documents describe". Compressed here to the half that is
still a live question; the argument about that sentence is settled and is in git.

## The short answer

**Yes, and the strongest evidence is that it is built and passing.** Every gate
the assessment set has been met — 144MB against 373MB, 3.2ms a page against
27ms, five of the app's own Rust modules compiled unchanged with forty-four of
their own tests. "Is it a fit" was a live question in the spring; it is now
mostly a question answered by construction.

What is left is not a fit question at all. It is a **platform-coverage
question** — Moonowl ships installers for Windows and Linux — and a **maturity
question**, which is a schedule risk rather than a design one.

The right way to read QRnew's sentence is as a compliment to how little QRnew
had to change rather than a doubt about Moonowl. The two apps went through the
same door for different reasons: QRnew because the door was open, Moonowl
carrying a PDF renderer, a window manager, a theme engine and eleven thousand
lines of interface through it, because every other door was measured and found
shut.

## Three things that fit structurally, not by luck

*The renderer was already behind one door.* `viewer.ts` is the only file that
imports pdf.js and `api.ts` the only door into Rust, so swapping the renderer
needed no change to either — which is what made the pdfium prototype cheap to
run and cheap to reject.

*The Rust side never knew about Tauri.* `theme.rs`, `settings.rs`, `keys.rs`,
`library.rs` and `watch.rs` carry no `#[tauri::command]` between them; only
`lib.rs` did. They are mounted rather than copied, and the day one grows a Tauri
dependency the experiment stops compiling.

*The layout was already a pure function.* `relayout`, `rows`, the two binary
searches and the mounting window are arithmetic over numbers the page already
holds, so they ported line for line into a struct with no renderer, no widget
and no window in it — and became testable in the process.

## What does not fit, stated plainly

**The window story is off the documented path.** `shell.rs` owns
`BlitzApplication` directly and is written against public fields rather than a
supported API, because `DioxusNativeApplication::add_window` does not do what its
name says and `use_window_event` is closed to a shell of our own. It works — two
documents, the cascade, the Dock menu, one instance over a Unix socket, the
quit-versus-close rule — and most of the *rules* turned out to need no window at
all and are `windows.rs` with fourteen tests. But it is the highest-risk item in
the tree and it has been shown to work on macOS and nowhere else.

**Eighteen upstream faults, all worked around, most with a test that fails the
day they are fixed.** None is a blocker; all are the cost of alpha software, and
the shape to expect is "found in a day, worked around in a day". The list is at
the end of `PROGRESS.md`.

**The test apparatus is rewritten, not ported.** The item most likely to be
underestimated by anyone reading the progress and not the plan. It is done, and
better than what it replaces on one axis — `screenshot()` means rendering can be
tested, which `npm test` never could — but "we replaced the safety net first" is
only comfortable in retrospect.

**The binary doubles.** 6.2MB against 12MB plus 7.2MB of pdfium. The brief
permits this explicitly as a price for memory, and the price is paid.

## The one thing genuinely still open

**Windows and Linux.** Four separate risks live here and they are not the same
risk:

| | what is unknown | how bad if it fails |
| --- | --- | --- |
| Stylo, Parley, fontique, Taffy | do they behave on engines this is not developed on | probably small; findable with no GPU |
| `vello_hybrid` on common Linux GPUs | is it smooth on an Intel iGPU | `vello_cpu` is built and is the fallback; if neither is smooth, **stop** |
| the shell of our own | winit's window lifecycle on three platforms | **stop** — multi-document is not negotiable |
| single instance | Unix socket has no Windows equivalent; a named pipe has no std type | contained; a known amount of work |

Only the third and fourth are structural. The first is the cheapest thing left
on the whole list to find out about, and the CI job exists to answer it.

## Recommendation

**Continue, and do the cheap platform work before the expensive feature work.**
The two structural risks left are both about platforms; finishing a feature on
macOS and *then* discovering the shell does not hold on Windows is the worst
available sequence. What remains of that list:

1. **File the focus fault upstream**, with the `is_focussable()` pointer in
   `PROGRESS.md`'s upstream list. It is the fault this tree has paid for four
   times and the fix is one call in a file that already has the predicate.
   Written up and not filed: that step needs an account rather than a commit.
2. **Get the shell onto Windows and Linux**, in that order of doubt. The named
   pipe for single instance is a known amount of work; the window lifecycle is
   the unknown one. Until the CI job has been green once, "nothing here has run
   on Windows or Linux" is still the true sentence.
3. **Then Phase 4** — the decision, with the same shape as the pdfium write-up.
