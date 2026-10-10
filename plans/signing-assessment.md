# Signing a PDF: what it would take

Asked while reading: *any chance to add document signing?* The short answer is
that "signing" is two unrelated features wearing one word, and Moonowl is a
couple of days from one of them and a month from the other.

## The two things

|  | **Visible signature** | **Cryptographic signature** |
| --- | --- | --- |
| What it is | A picture of a name, placed on a page | A `/Sig` dictionary holding a detached CMS blob over the file's bytes |
| What it proves | Nothing. It is ink. | Who signed, and that nothing has changed since |
| Who does it | Preview's *Markup → Sign* | Acrobat's green tick |
| What people actually ask for | Usually this | Sometimes this, and they say so |

Most people asking to "sign a PDF" want to put their name on a contract and
send it back. That is the first column, and it is a drawing feature.

## The visible signature: a day or two

Everything hard about it is already built, for markup:

- **Writing into the document.** `markup::edit` under `Viewer::write_step`
  writes off the thread that draws, atomically, keeps the file as it was in
  the config directory for undo (`markup::Before`), tells the watcher the
  burst is ours, and reloads the document through the path a LaTeX recompile
  already uses.
- **Placing something on a page.** The markup path already turns a rectangle
  on screen into PDF points on the right page, through the rotation and the
  crop (`markup::Space`).
- **The annotation itself.** `create_stamp_annotation` and
  `PdfPageImageObject` are both in `pdfium-render`; a stamp with an image in
  it is exactly what Preview writes.

What is left is the part the reader sees: capture the signature (trackpad
drawing, a photographed sheet of paper thresholded to transparency, or a typed
name in a script face), keep two or three of them in the config directory, and
let one be dragged onto a page. Also a date field and a plain text field,
because the form under a signature usually wants both.

One caveat worth stating up front: **it cannot be removed afterwards** by the
same route markup can, unless it goes through the same rebuild-from-backup
path. Decide that before shipping it, not after.

## The cryptographic signature: not reachable from here

### pdfium will not write one

pdfium's entire signature surface is eight functions and all eight read:
`FPDF_GetSignatureCount`, `FPDF_GetSignatureObject`, and six
`FPDFSignatureObj_Get*` getters for the contents, byte range, sub-filter,
reason, time and DocMDP permission. There is no writer, and there is no
plan to add one.

Worse, the save is wrong for this by construction. `save_to_bytes` is
`FPDF_SaveAsCopy` with flags hard-coded to 0 — a **full rewrite** of the
document. A signature covers a byte range of a specific file; rewriting the
file is precisely what a signature exists to detect. So even a signature
written by some other means would be destroyed the first time a reader marked
a passage.

The retired Tauri app was no better off. pdf.js's `saveDocument()` *does* write
a proper incremental update, but `saveNewAnnotations` in the worker has cases
for `FREETEXT`, `HIGHLIGHT`, `INK`, `STAMP` and `SIGNATURE` only — and its
`SIGNATURE` case is a drawing, not a `/Sig`. Neither renderer can do this.

### So it would need its own writer

The parts, in the order they bite:

1. **An incremental-update writer.** Append to the file rather than rewriting
   it: a new `/Sig` object, an `/AcroForm` with `/SigFlags 3`, a widget
   annotation for the signature field, an updated page object, a fresh xref
   section and trailer pointing back at the old one. Roughly the shape
   `markup.rs` describes and deliberately does not do.

2. **The `/ByteRange` chicken-and-egg.** The signature covers the whole file
   *except* the hex string holding the signature. So: write the file with a
   placeholder of exactly the right length, compute the two ranges, hash them,
   sign the hash, and splice the result back into the reserved space without
   moving a single byte. Get the reservation wrong by one and every verifier
   rejects the file.

3. **A detached CMS/PKCS#7 blob**, `SubFilter /ETSI.CAdES.detached` for
   anything modern, carrying the signer's certificate chain.

4. **A key, and this is the hard half.** Steps 1–3 are fiddly but finite —
   call it a week or two of careful work against a corpus of verifiers. Step 4
   is a product decision:

   - *Where does the key come from?* macOS Keychain, Windows CryptoAPI, a
     PKCS#12 file the reader types a password for, a PKCS#11 smartcard, or a
     national eID. Each is a separate platform integration.
   - *Whose trust store decides it is valid?* A self-signed certificate makes
     Acrobat show a yellow warning triangle, which reads to most people as
     *this document is suspicious*. A signature that alarms the recipient is
     worse than no signature. Being trusted by default means the Adobe
     Approved Trust List, which is a commercial arrangement, not a code change.

5. **A timestamp**, RFC 3161, from a trusted authority — otherwise the
   signature dies with the certificate. And for archival validity (PAdES
   B-LTA), embedded revocation data on top.

6. **Verification**, which is a second feature of similar size: chain
   building, revocation checking, and telling the reader in one honest line
   whether the document they are holding is what was signed. Showing a green
   tick you have not earned is the one outcome worse than showing nothing.

**Estimate: 2–4 weeks**, most of it in 4 and 6, and a permanent maintenance
tail as certificate formats and trust lists move.

## Ways around it

- **Ship the visible signature and say what it is.** Call it "Sign" and let it
  be a drawing, the way Preview does. This is what almost everyone wants.
- **Read signatures without writing them.** pdfium's eight getters are enough
  to say *this document is signed by X, and the bytes still match the range
  that was signed* — which is genuinely useful, ships in a day, and makes no
  promises the app cannot keep. Worth doing on its own.
- **Hand off to a signer.** Detect that the reader wants a cryptographic
  signature and open the document in whatever the platform provides, or shell
  out to a signing tool they already have. Honest, cheap, and nobody's trust
  chain becomes our problem.
- **Build it on crates, not on pdfium.** If it is ever really wanted: `lopdf`
  for the incremental update and a CMS crate for the blob, with the key coming
  from the platform store. This is the real version of the estimate above.

## What was built

**The visible signature is built**, in `src/sign.rs`. It follows the
recommendation below with one deliberate departure and one caveat settled the
other way.

*Ink, not a stamp.* `/Ink` is the specification's own annotation for a mark
made by hand; `create_stamp_annotation` with an image in it is a rubber stamp.
Ink is vector — a signature read at 400% is drawn from its strokes rather than
resampled — it needs no rasteriser, and every other reader shows it as what it
is and offers to delete it.

*Which settles the caveat above.* "It cannot be removed afterwards" is true of
this app and not of pdfium: `FPDFPage_RemoveAnnot` is one call, the same one a
highlight comes out through. So the Sign window lists what is already on the
document with a bin beside each row, and the feature ships with the caveat
answered rather than carried.

*A signed document is asked about, not refused.* pdfium's save is a full
rewrite, so ink into a cryptographically signed document ends the signature it
carried. The reader is told once, before it happens, in a sentence that names
the consequence rather than the mechanism.

*And the line beside it.* "Also a date field and a plain text field, because
the form under a signature usually wants both" — both are there, as one field
with a **Today** button beside it, placed by the same click that places a
signature. They go in as a `/Stamp` holding a Helvetica text object rather than
the `/FreeText` the specification has for this: `pdfium-render` exposes
`objects_mut()` on ink and stamp alone, and a free text annotation with an
empty appearance stream is text no reader draws, pdfium's own included.

*And the document's own signatures are read.* The recommendation below asks
for this beside the writing and it is built: the Sign window lists every
`/Sig` the document carries, before the pad and under the sentence saying what
this feature is not.

**One line of the recommendation is wrong, and reading them is what showed
it.** "pdfium's eight getters are enough to say *this document is signed by X,
and the bytes still match the range that was signed*" — they are not.

* *There is no name getter.* The signer's name is in the certificate inside the
  PKCS#7 blob. Reading it means parsing DER and choosing between several common
  names in a chain, and a guess about who signed a contract is worse than
  silence.
* *There is no digest check.* "The bytes still match" is a hash over the byte
  ranges compared against the message digest in that same blob.
* *Two of the eight are unreachable anyway.* `pdfium-render 0.9.3` wraps
  neither `FPDFSignatureObj_GetByteRange` nor `GetSubFilter`, and keeps both
  `PdfSignature`'s handle and `PdfDocument`'s `pub(crate)` — so there is no
  door onto them. The byte range is what would have answered "was anything
  appended after this was signed", which is the one useful thing obtainable
  with no crypto at all.

What the getters *are* enough for is four certain facts: that something was
signed, when it says it was, why, and whether it forbids changes. That is what
is shown.

**And the first of those four was a bug.** `FPDF_GetSignatureCount` counts
every `/FT /Sig` field in the `/AcroForm` whether or not anybody has signed it
— a blank one is the line at the foot of a contract, put there by whoever wrote
the contract. So the warning above fired on documents nobody had signed,
including this repository's own `tests/fixtures/signed.pdf`, which is a field
and not a signature. `/Contents` tells them apart.

`dormant/experiments/PROGRESS.md` has the long form, including the two faults
the feature turned up in code that was already there.

Everything below stands as written.

## Recommendation

Build the visible signature — it is a natural neighbour of the markup that is
already there and it reuses the whole write path. Add signature *reading*
alongside it, because the API is free and "signed by X, intact" is a real
answer to a real question.

Treat cryptographic signing as its own decision, made later and on its own
merits. It is not blocked by anything in this codebase; it is blocked by the
fact that a signature nobody's software trusts is not a signature.
