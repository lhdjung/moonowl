# Audit, 9 October 2026: what is open

What is left of the audit of `5ab3050`. Everything else it found is fixed, one
commit per fix, on `page-field-selection` (`7c6a322` to `45d6b59`). Each item
below waits on the trigger in its heading.

## Before the first tagged release

### The Rust crates' licence notices are not shipped
pdfium's notices travel with the app; the crates' do not. THIRD-PARTY.md says
only that the texts are "in each crate".
- MIT and Apache-2.0 want the notice with binary copies.
- `stylo`, `selectors` and `cssparser` are MPL-2.0. MPL §3.2(a) wants
  recipients told where the source is.
- THIRD-PARTY.md's "nothing here has to be shared back" is too broad for MPL:
  modified MPL files must be.

**Fix:** generate a notices file with `cargo-about` into `licenses/`, and ship
it with the bundles.

## After the first `v*` release

### Nightly is still the "latest" release
`nightly.yml` publishes with `--latest`, because `nightly` is the only release
there is and a prerelease would turn every `releases/latest/download/…` link
in the README into a 404. Once a tagged release exists:
- publish nightly as a prerelease, so the links reach the tagged release;
- that also closes the moment between `gh release delete nightly` and the
  `edit`, when a cancelled run leaves no "latest" at all.

## At the next fork bump

- Rebasing `moonowl-2` orphans the revs older commits pin, which then build
  only until GitHub garbage-collects them. Tag each pinned rev.
- `winit = "=0.31.0-beta.3"` pins a beta exactly.
- There is no Dependabot or `cargo audit` step. `Cargo.lock` showed nothing
  yanked or known-vulnerable at the audit.

## When there is a Windows machine

**Printing** (`print.rs`) has never run on Windows:
- `PrintDlgW` returning FALSE is taken as Cancel, but it also returns FALSE on
  errors such as `PDERR_NODEFAULTPRN`, so Print does nothing. Check
  `CommDlgExtendedError()`.
- `count.max(1) as u16` wraps past 65,535 pages.
- Landscape pages are not auto-rotated, unlike the Mac path.

`fill_in_place` in `markup.rs` (`45d6b59`) has been built for Windows but
never run.

## When it is noticed in use

- **Selection waits on pdfium on the UI thread.** `text_on` →
  `Document::text_of` takes the blocking `library()` lock on a press, a drag,
  a word, line or paragraph click, ⌘A, and `selected_areas`. A press on a page
  that just scrolled in waits for that page's render. `try_text_of` would mean
  holding the gesture until the text lands and replaying it; reading each
  page's text on a thread as it mounts is simpler.
- **The selection and link shader scales with runs × pixels.**
  `regions.wgsl` tests every run for every invocation, with no early exit. A
  selected page of 60 lines, or a bibliography with 200 links, comes to
  hundreds of millions of tests per pass. Fix: a run index per shelf, or one
  dispatch per run.
- **The Contents list is not virtualised**: one button per heading.
- **Leftover temp files.** `atomic_write` leaves `.<name>.<pid>.<n>.tmp`
  beside the PDF if the process dies between create and rename, and nothing
  sweeps it. There is no directory fsync after the rename either, which costs
  durability only.

## When the tests are next worked on

- **Tests on the real clock.** `Reader::wait_until` (`harness.rs`) is used in
  `tests/chrome.rs`, `paint.rs`, `reader.rs`, `landing.rs`, `select.rs` and
  `watch.rs`. Several check that something does *not* happen within N seconds,
  which costs about 10s a run and proves little on a loaded runner; the
  positive ones can flake on slow CI. The cause: `BAR_LASTS`, `PILL_LASTS`,
  the cursor timer and `STILL_TICK` read `Instant` directly, and the harness
  has no clock to swap in. `tests/watch.rs` sleeps and retries because the
  watcher never signals that it is ready.
- **Gaps:**
  - No way to inject an I/O failure into a markup write (disk full, refused
    rename, `Before::take_to` failing).
  - `session.rs`, `single.rs` (one test) and printing are untested.
  - Nothing tests a document replaced mid-write, or the flush of `SETTLE` /
    `ZOOM_SETTLES` at window close.

## When a file is next touched

**Comments that are not in the present tense.** `src` had 37 "used to", 16
"before this", 21 "no longer", and about 70 references to the retired app
(`viewer.ts`, Tauri, pdf.js). The worst:
- `styles.rs`: 25 lines on the earlier letter-spacing approach, above `body`.
- "This experiment" wording, now that the experiment is the app: `keymap.rs`,
  `single.rs`, `page.rs`, `app.rs`.
- `write_step`'s doc comment opens with a paragraph that belongs to
  `write_file`/`offload`, and a `// ponytail:` line splits it.

Comment-to-code ratios at the audit, for scale: `app.rs` 0.46, `harness.rs`
0.57, `store.rs` 0.53, `page.rs` and `pdfium.rs` ~0.45, `Cargo.toml` 1.38;
about 47% of `styles.rs` is block comments.

## Decided against

- **Icon-only controls.** Page previous/next and the find card's previous,
  next and close carry no label. They sit beside a field or a query that says
  what they move through, and a label would crowd the bar.
- **A prerelease nightly before there is a tagged release**: see above.
