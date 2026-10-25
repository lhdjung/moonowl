# Audit, 9 October 2026: what is open

What is left of the audit of `5ab3050`. Everything else it found is fixed, one
commit per fix, on `page-field-selection` (`7c6a322` to `45d6b59`). Each item
below waits on the trigger in its heading.

## Follow-up check, 10 October 2026: the fixes, reviewed

A second pass over the commits the audit produced (`7c6a322` to `a91ec4e`),
read against `a91ec4e`. The refactors (the `app/` split, `Event`, the
toolbar memo, the dialogs) change no behaviour; what the fixes themselves got
wrong or left out is fixed, one commit per item, `61c2bd2` to `1e3bd35`,
each with a test where one can fail. What is left:

- **CI has still not run.** The checksum step, the release retry, the
  nightly guard and the tokens are fixed on paper; the first push to main
  and the first dispatched release are their test.
- *Done (`4891b65`).* `Bar::of` read `library.toml`'s time on every scroll
  frame while the Open menu was open; the list is now read when it opens.
- *Done (`42c8ddf`).* The three dialogs (Details, Sign, the password) read
  the viewer as they rendered; each now renders from a memo.

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

- **The Contents list is not virtualised**: one button per heading.
- *Done: the next write sweeps them once they are an hour old, and the folder
  is synced after the rename.* **Leftover temp files.** `atomic_write` leaves `.<name>.<pid>.<n>.tmp`
  beside the PDF if the process dies between create and rename, and nothing
  sweeps it. There is no directory fsync after the rename either, which costs
  durability only.

## When the tests are next worked on

- *Done.* **Tests on the real clock.** The harness stops the clock the
  reader's timers count by (`emit::now`, `emit::after`), and a test moves it
  with `Reader::later` instead of waiting. `wait_until` is left for what
  happens on another thread: a page being read, a page drawn, the file
  system. `tests/watch.rs` still saves and retries: on macOS the file events
  start arriving some time after `notify` says the watch is up, so a signal
  that the watch is up would not remove the wait.
- **Gaps:**
  - *Partly:* a folder that refuses the new file is tested
    (`a_folder_that_refuses_the_write_keeps_the_highlight`). A full disk and
    `Keeper::take` failing are not: both need a way to make one write fail
    inside the process.
  - *Partly:* `single.rs` has a second test (silence from the holder is not
    a hand-over). `session.rs` and printing need a window.
  - *Done:* a zoom still settling is on the disk once a quit has flushed
    (`a_pinched_zoom_is_on_the_disk_once_the_quit_has_flushed`). A document
    replaced behind the reader's back before the write is covered by
    `a_refused_highlight_is_kept_with_no_window_to_land_in`; one replaced
    between the read and the rename is not.

## When a file is next touched

*Done: the comments in `src` were rewritten.* **Comments that are not in the present tense.** `src` had 37 "used to", 16
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
