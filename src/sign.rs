//! Signing a document, in the sense almost everybody means by the word.
//!
//! A reader draws their name once, keeps it, and drops it onto a page as a
//! `/Subtype /Ink` annotation — the specification's own annotation for
//! something drawn by hand, with the strokes in `/InkList`.
//!
//! **It is ink, and it proves nothing.** `signing-assessment.md` is the long
//! form: "signing" is two unrelated features wearing one word, and a
//! *cryptographic* signature — a `/Sig` holding a detached CMS blob over a
//! byte range — is the other one. Neither renderer in this repository can
//! write one, and the hard half is not the writing but the trust. The words
//! the window uses say so rather than implying the green tick.
//!
//! What that costs is [`Standing::rewrites`]: a document already carrying a
//! cryptographic signature can still be signed with ink, and doing so breaks
//! the signature — the save is a full rewrite. The reader is told once, before
//! it happens.
//!
//! **Ink rather than a stamp**, which is what the assessment reached for and
//! what Preview writes. Three reasons, and the third decided it: it is vector,
//! so a signature read at 400% is drawn from its strokes; it needs no
//! rasteriser, and the only one in this crate is behind the `harness` feature
//! and deliberately not in the binary; and `/Ink` is *the annotation that
//! means this*, so every other reader shows one as what it is and offers to
//! delete it.
//!
//! A signature is kept in the config directory as one TOML file, the way a
//! theme is: a small thing somebody may want to copy to another machine.
//! The strokes are normalised **by height** — y runs 0 to 1 and x runs to
//! however wide the hand made it — so `aspect` is a real number and one
//! multiplication does both axes. Normalising each axis into a unit box reads
//! correctly and throws the shape away: every signature came back square.

use pdfium_render::prelude::{
    PdfColor, PdfDocument, PdfPageAnnotationCommon, PdfPageObjectCommon, PdfPageObjectsCommon,
    PdfPagePathObject, PdfPoints, PdfRect,
};
use serde::{Deserialize, Serialize};

use crate::render::Rect;

/// How wide a stroke is drawn, as a fraction of the signature's height.
///
/// A fraction rather than a number of points, because a signature dropped
/// small and a signature dropped large should look like the same hand. At
/// 1/28th, a signature 40 points tall is drawn with a 1.4pt nib, which is
/// about what a fine liner leaves on paper.
const NIB: f32 = 1.0 / 28.0;

/// The thinnest a nib may get, in points. A signature dropped very small would
/// otherwise be drawn with a stroke pdfium renders as nothing at all.
const THINNEST: f32 = 0.4;

/// What a signature is drawn in, unless the reader says otherwise: the blue of
/// a ballpoint rather than black, which is what people actually sign in and
/// what tells a signature apart from the printing under it at a glance.
pub const INK: &str = "#1c3f94";

/// One signature, as it is kept on disk and as it is drawn.
///
/// The strokes are **normalised by height**: y runs 0 to 1 downwards and x runs
/// 0 to however wide the signature is against its own height, so a name twice as
/// wide as it is tall has x running 0 to 2 and [`Signature::aspect`] answers 2.
///
/// **A unit box in both axes was the first shape and it was wrong**: stretching
/// each axis to 0-1 separately throws the *shape* away, so every signature came
/// back square. One scale for both axes keeps it the shape it was written.
///
/// Height carries the 1 because height is what a reader means by how big a
/// signature should be — see [`crate::app::HAND_HEIGHT`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Signature {
    /// What the reader calls it. Several are allowed — a full name and a set
    /// of initials is the ordinary pair — so they need telling apart.
    pub name: String,
    /// Its id, which is its file name without the extension. Empty for a
    /// signature that has not been saved yet, which is what [`save`] mints
    /// one for.
    #[serde(default)]
    pub id: String,
    /// The strokes, each a run of points the pointer passed through with the
    /// button down. A stroke of one point is a dot and is kept: a dot over an
    /// i is a stroke of one point.
    #[serde(default)]
    pub strokes: Vec<Vec<[f64; 2]>>,
}

impl Signature {
    /// Whether there is anything to draw. A signature with no strokes is a pad
    /// nobody drew on, and it is refused rather than saved: an empty file in
    /// the list is a row that does nothing when it is chosen.
    pub fn is_empty(&self) -> bool {
        self.strokes.iter().all(|stroke| stroke.is_empty())
    }

    /// The box the strokes actually occupy: left, top, right, bottom. `None`
    /// for a signature with no points in it at all.
    fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        self.strokes.iter().flatten().fold(None, |bounds, point| {
            let (x, y) = (point[0], point[1]);
            Some(match bounds {
                None => (x, y, x, y),
                Some((left, top, right, bottom)) => {
                    (left.min(x), top.min(y), right.max(x), bottom.max(y))
                }
            })
        })
    }

    /// How wide it is against its height, which is what decides the box it is
    /// dropped into: a signature is drawn to a height and takes whatever width
    /// its own shape asks for.
    ///
    /// One, for a signature with no extent — a single dot, or nothing at all.
    /// A ratio of zero would be a box with no width, and an annotation with no
    /// width is drawn by nothing.
    pub fn aspect(&self) -> f64 {
        match self.bounds() {
            Some((left, top, right, bottom)) if bottom > top && right > left => {
                (right - left) / (bottom - top)
            }
            _ => 1.0,
        }
    }

    /// The strokes moved to the origin and scaled so that they are one unit
    /// tall, keeping their shape.
    ///
    /// This is done on the way *in*, at [`save`], rather than on the way out —
    /// so the file holds a signature already trimmed, and what is on disk is
    /// what will be drawn. A signature normalised on every use would be a file
    /// whose meaning depended on the code reading it.
    ///
    /// **One scale for both axes**, which is the whole of what this function
    /// gets right and its first version got wrong. See the note on
    /// [`Signature`].
    pub fn trimmed(&self) -> Signature {
        let Some((left, top, right, bottom)) = self.bounds() else {
            return self.clone();
        };
        let down = bottom - top;
        let across = right - left;
        // A signature that is one straight horizontal line has no height to
        // divide by. It is scaled by its width instead and set on the middle
        // line, which is where a line drawn across a pad belongs — the
        // alternative is dividing by nothing and writing infinities into
        // somebody's file.
        let (scale, middle) = if down > f64::EPSILON {
            (down, 0.0)
        } else if across > f64::EPSILON {
            (across, 0.5)
        } else {
            // A single dot: nothing to scale by, and nothing that needs it.
            (1.0, 0.0)
        };
        Signature {
            strokes: self
                .strokes
                .iter()
                .map(|stroke| {
                    stroke
                        .iter()
                        .map(|point| [(point[0] - left) / scale, (point[1] - top) / scale + middle])
                        .collect()
                })
                .collect(),
            ..self.clone()
        }
    }
}

/// One signature already in a document, as it is read back out of it.
///
/// The neighbour of [`crate::markup::Mark`], identified the same way: the page
/// and where it sits among that page's annotations, good for as long as the
/// document is the one it was read from. Every write is followed by a reopen, so
/// an index never crosses one.
///
/// **The strokes are not read back.** What the interface needs is that there is
/// a signature, where it is, and who it says made it. Nothing draws these — the
/// renderer draws them, because they are in the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Written {
    /// Ink: a name drawn by a hand, in an `/Ink` annotation.
    Hand,
    /// Type: a date or a line of text, in a `/Stamp`. See [`place_text`] for
    /// why a stamp and not the `/FreeText` the specification has for it.
    Line,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    /// Which of the two things this reader writes it is.
    pub kind: Written,
    /// One-based, as every page number in this crate is.
    pub page: usize,
    /// Where it sits among the annotations of that page.
    pub index: usize,
    /// Its box, in the page's own points counting from the top left.
    pub at: Rect,
    /// What it is called in the list: for ink, the annotation's creator, which
    /// is the signature's name; for a line, the words themselves. Both are set
    /// by the write and left alone for an annotation this reader did not put
    /// there.
    pub by: String,
}

/* ------------------------------------------------------------ the store */

/// Where the signatures live inside a config directory: one file each, beside
/// the themes.
///
/// **The directory is passed in rather than asked for**, which is the shape
/// `theme::load_all` and `settings::load` already have and is not a matter of
/// taste: `config_dir()` reads an environment variable, one process has one of
/// those, and `cargo test` runs a file's tests on several threads at once. A
/// store that reached for the ambient directory would have every test in this
/// file writing into whichever one the last of them set — which is exactly
/// what happened, and it looked like a delete that did not delete.
pub fn dir(config: &std::path::Path) -> std::path::PathBuf {
    config.join("signatures")
}

/// Every signature the reader has kept, by name.
///
/// **A file that will not parse is skipped rather than fatal**, which is
/// `theme::load_all`'s rule and is right for the same reason: these are files
/// somebody may have edited by hand, and one bad one must not take the list
/// with it. Sorted by name, because the order of a directory is nobody's
/// intention.
pub fn load_all(config: &std::path::Path) -> Vec<Signature> {
    let Ok(entries) = std::fs::read_dir(dir(config)) else {
        return Vec::new();
    };
    let mut out: Vec<Signature> = entries
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|kind| kind == "toml"))
        .filter_map(|entry| {
            let body = std::fs::read_to_string(entry.path()).ok()?;
            let mut signature: Signature = toml::from_str(&body).ok()?;
            // The id is the file name, whatever the file says — the same rule
            // `theme.rs` uses, and for the same reason: a file copied to
            // another machine under a new name is that new signature, and two
            // files claiming one id would be one signature that cannot be
            // deleted.
            signature.id = entry
                .path()
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_default()
                .to_string();
            (!signature.id.is_empty() && !signature.is_empty()).then_some(signature)
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    out
}

/// Keep one, trimmed, and answer what it was stored as.
///
/// A signature with no id is given one made from its name, which is what makes
/// the file findable by somebody looking in the directory. Two signatures
/// called the same thing get `-2`, `-3` and so on rather than one quietly
/// replacing the other.
pub fn save(config: &std::path::Path, signature: &Signature) -> Result<Signature, String> {
    if signature.is_empty() {
        return Err("There is nothing drawn to keep.".into());
    }
    let mut stored = signature.trimmed();
    if stored.name.trim().is_empty() {
        stored.name = "Signature".to_string();
    }
    if stored.id.trim().is_empty() {
        stored.id = mint(config, &stored.name);
    }
    let body = toml::to_string_pretty(&stored).map_err(|e| e.to_string())?;
    let file = dir(config).join(format!("{}.toml", stored.id));
    crate::atomic_write(&file, body.as_bytes())?;
    Ok(stored)
}

/// Take one off the list, and off the disk.
pub fn forget(config: &std::path::Path, id: &str) -> Result<(), String> {
    if id.trim().is_empty() || id.contains(['/', '\\']) || id.contains("..") {
        return Err("That is not a signature.".into());
    }
    std::fs::remove_file(dir(config).join(format!("{id}.toml"))).map_err(|e| e.to_string())
}

/// A file name from a name: lower case, spaces to hyphens, nothing that is not
/// a letter or a digit, and a number on the end if the name is taken.
fn mint(config: &std::path::Path, name: &str) -> String {
    let stem: String = name
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    let stem = if stem.is_empty() {
        "signature".to_string()
    } else {
        stem
    };
    let taken = |id: &str| dir(config).join(format!("{id}.toml")).exists();
    if !taken(&stem) {
        return stem;
    }
    // Two is where a second one starts, which is how anybody numbers a second
    // copy of anything. The ceiling is there so that a directory somebody has
    // filled by hand cannot spin.
    (2..1000)
        .map(|nth| format!("{stem}-{nth}"))
        .find(|id| !taken(id))
        .unwrap_or(stem)
}

/* ------------------------------------------------- into the document */

/// What signing this document would mean, asked before it is offered.
///
/// [`crate::markup::Standing`] answers the first two of these already — the
/// document is writable, or it is not, and it carries a cryptographic
/// signature, or it does not — and this adds the sentence that turns the
/// second into something a reader can act on.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Standing {
    /// Whether ink can go into the file at all.
    pub into_file: bool,
    /// Why not, when it cannot. `markup`'s own sentence.
    pub refused: String,
    /// **What signing costs a document that is already signed.** The save is
    /// `FPDF_SaveAsCopy`, a full rewrite, and a cryptographic signature covers
    /// a byte range of a specific file — so writing ink into a signed document
    /// breaks the signature that was there. Said once, before it happens,
    /// because it is their document and the alternative is finding out from
    /// somebody else's verifier.
    pub rewrites: bool,
}

/* ------------------------------------- what the document is already signed with */

/// One cryptographic signature the document already carries, as pdfium reads
/// it.
///
/// **The other column of `signing-assessment.md`, read rather than written.**
/// Nothing here can make one; pdfium's whole signature surface is read-only,
/// which is a reason to read them rather than a reason to say nothing.
///
/// **The assessment said the eight getters are enough for "signed by X, and the
/// bytes still match the range that was signed". They are not**, and this is
/// what they are enough for:
///
/// * *There is no name getter.* The signer's name lives in the certificate
///   inside the PKCS#7 blob, and reading it means parsing DER and deciding
///   which of several common names in a chain is the subject's. A guess there
///   is worse than silence, so nothing here guesses.
/// * *There is no digest check.* "The bytes still match" means hashing the
///   byte ranges and comparing against the message digest in the blob, which
///   is the same DER parse plus a hash. Not done, and not claimed.
/// * *Two of the eight getters are unreachable.* `FPDFSignatureObj_GetByteRange`
///   and `GetSubFilter` exist in the bindings and `pdfium-render 0.9.3` wraps
///   neither, and it keeps `PdfSignature`'s handle and `PdfDocument`'s handle
///   both `pub(crate)` — so there is no door onto them from outside the crate.
///   That is what would have answered "was anything appended after this was
///   signed", which is the one useful thing obtainable without any crypto at
///   all.
///
/// What is left is four facts, every one of them certain, and the first is the
/// one worth having.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Seal {
    /// **Whether anything has actually been signed.** A `/FT /Sig` field with
    /// no `/V` is a *place* for a signature — the blank line at the foot of a
    /// contract — and `FPDF_GetSignatureCount` counts it exactly the same as a
    /// signed one. Telling them apart is `/Contents`, and it is the difference
    /// between warning a reader that they are about to break a signature and
    /// warning them about a signature nobody has made.
    pub filled: bool,
    /// When it says it was signed: the `/M` entry, in words. Empty when it
    /// gives none.
    pub when: String,
    /// Why, in the signer's own words. Empty when they gave none.
    pub reason: String,
    /// The DocMDP level, when it sets one: 1 permits no change at all, 2
    /// permits filling in forms and signing, 3 permits annotations as well.
    /// `None` is the ordinary case and means the signature certifies nothing
    /// beyond itself.
    pub locks: Option<u8>,
}

impl Seal {
    /// The one line the window shows under it. Says only what is known, and
    /// says plainly when that is nothing.
    pub fn says(&self) -> String {
        if !self.filled {
            return "waiting to be signed".to_string();
        }
        let mut said = Vec::new();
        if !self.when.is_empty() {
            said.push(format!("signed {}", self.when));
        }
        if !self.reason.is_empty() {
            said.push(self.reason.clone());
        }
        match self.locks {
            // Worth a sentence of its own: a level 1 certification says the
            // document is not to be changed by anybody, which is a stronger
            // claim than "somebody signed this" and is the one that makes ink
            // on top of it an actual disagreement.
            Some(1) => said.push("permits no changes".to_string()),
            Some(_) => said.push("certified".to_string()),
            None => {}
        }
        if said.is_empty() {
            "it says nothing about when or why".to_string()
        } else {
            said.join(" · ")
        }
    }
}

/// Every signature the document at `path` carries.
///
/// Opened here rather than asked of the open document, the way
/// [`crate::markup::standing`] already asks the disk: this is read once when a
/// window is opened and never in a frame, and a document that has been
/// released for a write has no pages to ask.
pub fn seals(path: &str, password: Option<&str>) -> Vec<Seal> {
    let _library = crate::pdfium::library();
    let Ok(pdfium) = crate::pdfium::pdfium() else {
        return Vec::new();
    };
    // With the password the document was opened with, or a locked signed
    // document lists no seals at all.
    let Ok(document) = pdfium.load_pdf_from_file(path, password) else {
        return Vec::new();
    };
    document
        .signatures()
        .iter()
        .map(|signature| Seal {
            filled: !signature.bytes().is_empty(),
            when: in_words(&signature.signing_date().unwrap_or_default()),
            reason: signature.reason().unwrap_or_default(),
            locks: signature
                .modification_detection_permission()
                .ok()
                .map(|level| {
                    use pdfium_render::prelude::PdfSignatureModificationDetectionPermission as Mdp;
                    match level {
                        Mdp::Mdp1 => 1,
                        Mdp::Mdp2 => 2,
                        Mdp::Mdp3 => 3,
                    }
                }),
        })
        .collect()
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// A PDF date in words: `D:20240314093000+01'00'` becomes `14 March 2024`.
///
/// Anything that is not that shape is handed back **unchanged** rather than
/// dropped or guessed at. A date is a fact the document is asserting, and the
/// two wrong answers are showing nothing and showing a date that is not the
/// one written down; showing the string as written is neither.
pub fn in_words(raw: &str) -> String {
    let digits = raw.strip_prefix("D:").unwrap_or(raw);
    let number =
        |from: usize, to: usize| digits.get(from..to).and_then(|s| s.parse::<usize>().ok());
    match (number(0, 4), number(4, 6), number(6, 8)) {
        (Some(year), Some(month), Some(day))
            if (1..=12).contains(&month) && (1..=31).contains(&day) =>
        {
            format!("{day} {} {year}", MONTHS[month - 1])
        }
        _ => raw.to_string(),
    }
}

/// Today, written the way somebody dates a form: `14 March 2024`.
///
/// **In UTC**, and there is no timezone crate here to do better. A date is
/// wrong by a day for a few hours either side of midnight in the far east and
/// the far west, and the answer to that is that it is offered as the *initial*
/// value of a field somebody can type into rather than stamped on their behalf.
/// Reaching for a timezone database to fill in a text field would be the wrong
/// trade.
pub fn today() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or(0);
    let (year, month, day) = civil(seconds.div_euclid(86_400));
    format!("{day} {} {year}", MONTHS[(month - 1) as usize])
}

/// Days since the epoch, as a date. Howard Hinnant's `civil_from_days`, which
/// is the standard answer and is exact for every date this will ever be handed.
fn civil(days: i64) -> (i64, u32, u32) {
    // Shifted so that the era begins on 1 March, which is what makes the leap
    // day the last day of the year and the whole thing branchless.
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let months = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * months + 2) / 5 + 1) as u32;
    let month = if months < 10 { months + 3 } else { months - 9 } as u32;
    (year_of_era + era * 400 + i64::from(month <= 2), month, day)
}

/// Ask the disk and the document, once.
pub fn standing(path: &str, encrypted: bool, sealed: bool) -> Standing {
    let markup = crate::markup::standing(path, encrypted, sealed);
    Standing {
        into_file: markup.into_file,
        refused: markup.refused,
        rewrites: markup.signed,
    }
}

/// The sentence a reader is shown before ink goes into a signed document.
///
/// One line, and it names the consequence rather than the mechanism: nobody
/// signing a contract needs to know what `FPDF_SaveAsCopy` is, and everybody
/// needs to know that the tick Acrobat was showing will stop being there.
pub const BREAKS_A_SIGNATURE: &str =
    "This document carries a digital signature. Adding ink to it rewrites the file, \
     which will make that signature stop verifying.";

/// Put a signature on a page.
///
/// `at` is where it goes, in the page's own points counting **from the top
/// left** — the space the selection, the links and the marks all work in, and
/// the space this crate flips exactly once, here.
///
/// The width of the box is ignored and computed from the height and the
/// signature's own shape, for the reason [`Signature::aspect`] gives: a
/// signature is drawn to a height and is whatever width its hand makes it.
/// Passing a box whose width is wrong would squash somebody's name.
pub fn place(
    path: &str,
    page: usize,
    at: Rect,
    signature: &Signature,
    ink: &str,
) -> Result<(), String> {
    if signature.is_empty() {
        return Err("There is nothing drawn to sign with.".into());
    }
    let [red, green, blue] = crate::palette::read_colour(ink).ok_or("That is not a colour.")?;
    crate::markup::edit(path, |document| {
        ink_one(document, page, at, signature, (red, green, blue))
    })
}

/// One signature on one page, inside the one open the gesture costs.
fn ink_one(
    document: &mut PdfDocument<'static>,
    page: usize,
    at: Rect,
    signature: &Signature,
    (red, green, blue): (u8, u8, u8),
) -> Result<(), String> {
    // **The page's height is read first and the page is then let go of**,
    // because a path object is made against the *document* and the document
    // cannot be borrowed while a page out of it is. So: ask the height, build
    // every stroke, and only then take the page mutably to hang them on.
    let space = {
        let page_ref = document
            .pages()
            .get(page.saturating_sub(1) as i32)
            .map_err(|e| format!("page {page}: {e}"))?;
        crate::markup::Space::of(&page_ref)
    };
    let height = at.height.max(1.0);
    // **One scale, and it is the height.** The strokes are height-normalised —
    // see [`Signature::trimmed`] — so x and y are measured in the same unit and
    // multiplying them by different numbers is what squashes a name.
    let width = height * signature.aspect();
    let nib = (height as f32 * NIB).max(THINNEST);
    let colour = PdfColor::new(red, green, blue, 255);

    // A point becomes a point on the page: across from the left edge of the
    // box, and *up* from the bottom of the page, because PDF counts from there
    // and everything else in this crate counts from the top. `at.top` is the
    // top of the box measured down from the top of the page and the point is
    // measured down from the top of the box, so the pair of them come off the
    // page height together.
    //
    // Through [`crate::markup::Space`], a point at a time, so a hand put on a
    // page the file turns is the right way up on the page as it is looked at.
    let onto = |point: &[f64; 2]| {
        let (x, y) = space.point_up(at.left + point[0] * height, at.top + point[1] * height);
        (x as f32, y as f32)
    };

    let mut paths = Vec::new();
    for stroke in &signature.strokes {
        let Some(first) = stroke.first() else {
            continue;
        };
        let (x, y) = onto(first);
        let mut path = PdfPagePathObject::new(
            document,
            PdfPoints::new(x),
            PdfPoints::new(y),
            Some(colour),
            Some(PdfPoints::new(nib)),
            None,
        )
        .map_err(|e| format!("the signature could not be drawn: {e}"))?;
        for point in stroke.iter().skip(1) {
            let (x, y) = onto(point);
            path.line_to(PdfPoints::new(x), PdfPoints::new(y))
                .map_err(|e| format!("a stroke was refused: {e}"))?;
        }
        // **A stroke of one point is a dot, and a path of one point draws
        // nothing.** pdfium emits a `m` with no `l` after it, which is a
        // current point and not a mark. So a lone point is given a second one
        // a hair away, which the round cap turns into the dot it was meant to
        // be.
        if stroke.len() == 1 {
            path.line_to(PdfPoints::new(x + 0.01), PdfPoints::new(y))
                .map_err(|e| format!("a stroke was refused: {e}"))?;
        }
        paths.push(path);
    }

    let mut page_ref = document
        .pages()
        .get(page.saturating_sub(1) as i32)
        .map_err(|e| format!("page {page}: {e}"))?;
    let mut annotation = page_ref
        .annotations_mut()
        .create_ink_annotation()
        .map_err(|e| format!("the signature could not be placed: {e}"))?;
    // An annotation with no bounds is not drawn — the same rule the highlight
    // path found, in the same words.
    annotation
        .set_bounds(box_of(&at, width, &space))
        .map_err(|e| format!("the signature could not be placed: {e}"))?;
    let _ = annotation.set_creator(&signature.name);
    for path in paths {
        annotation
            .objects_mut()
            .add_path_object(path)
            .map_err(|e| format!("a stroke was refused: {e}"))?;
    }
    Ok(())
}

/* --------------------------------------------------- a date, and a line of text */

/// How tall a line of text is put down, in points. A signature is 40 and this
/// is deliberately smaller: the name is the thing being said and the date under
/// it is a note about the name.
pub const LINE_HEIGHT: f64 = 11.0;

/// Put a line of text on a page — a date, a place, a name in type, whatever
/// the form under a signature is asking for.
///
/// `signing-assessment.md` names this beside the drawing and gives the reason
/// in six words: *the form under a signature usually wants both*. A signature
/// on its own is half of what people are actually asked to fill in.
///
/// **It is a `/Stamp` and not a `/FreeText`**, which is the annotation the
/// specification has for exactly this and is unreachable. `pdfium-render`
/// exposes `objects_mut()` on ink and stamp alone, and a free text annotation
/// with nothing in its appearance stream is text no reader draws — pdfium's own
/// included. So the text goes into a stamp's appearance stream as a real text
/// object in Helvetica, which every reader draws because there is nothing to
/// generate.
pub fn place_text(path: &str, page: usize, at: Rect, line: &str, ink: &str) -> Result<(), String> {
    let line = line.trim();
    if line.is_empty() {
        return Err("There is nothing typed to put on the page.".into());
    }
    let [red, green, blue] = crate::palette::read_colour(ink).ok_or("That is not a colour.")?;
    let line = line.to_string();
    crate::markup::edit(path, |document| {
        text_one(document, page, at, &line, (red, green, blue))
    })
}

/// One line on one page, inside the one open the gesture costs.
fn text_one(
    document: &mut PdfDocument<'static>,
    page: usize,
    at: Rect,
    line: &str,
    (red, green, blue): (u8, u8, u8),
) -> Result<(), String> {
    let space = {
        let page_ref = document
            .pages()
            .get(page.saturating_sub(1) as i32)
            .map_err(|e| format!("page {page}: {e}"))?;
        crate::markup::Space::of(&page_ref)
    };
    let size = at.height.max(1.0);
    // The font is asked for first: `fonts_mut` takes the document mutably and
    // the text object takes it again, so the token has to be out before the
    // second borrow begins. A token is a handle and not a borrow, which is why
    // this works at all.
    let font = document.fonts_mut().helvetica();
    let mut object = pdfium_render::prelude::PdfPageTextObject::new(
        document,
        line,
        font,
        PdfPoints::new(size as f32),
    )
    .map_err(|e| format!("the text could not be set: {e}"))?;
    object
        .set_fill_color(PdfColor::new(red, green, blue, 255))
        .map_err(|e| format!("the text could not be coloured: {e}"))?;
    // **`at.top` is the top of the line and a text object sits on its
    // baseline**, so the descender's worth of room comes off before the flip.
    // A fifth of the size is Helvetica's, near enough for a date on a form and
    // the difference nobody would see.
    //
    // Turned first, against the page's own `/Rotate`, so the line reads
    // across the page as it is looked at.
    let (x, baseline) = space.point_up(at.left, at.top + size * 0.8);
    if space.turned() != 0.0 {
        object
            .rotate_counter_clockwise_degrees(space.turned())
            .map_err(|e| format!("the text could not be placed: {e}"))?;
    }
    object
        .translate(PdfPoints::new(x as f32), PdfPoints::new(baseline as f32))
        .map_err(|e| format!("the text could not be placed: {e}"))?;
    // Asked of the object rather than guessed from the character count,
    // because Helvetica is proportional and a date is mostly digits and spaces.
    // A refusal falls back to half the size a character, which is about right
    // for type and is only ever the annotation's box.
    let width = object
        .width()
        .map(|points| points.value as f64)
        .unwrap_or(size * 0.5 * line.chars().count() as f64);

    let mut page_ref = document
        .pages()
        .get(page.saturating_sub(1) as i32)
        .map_err(|e| format!("page {page}: {e}"))?;
    let mut annotation = page_ref
        .annotations_mut()
        .create_stamp_annotation()
        .map_err(|e| format!("the text could not be placed: {e}"))?;
    annotation
        .set_bounds(space.up(&Rect {
            left: at.left,
            top: at.top - size * 0.3,
            width,
            height: size * 1.4,
        }))
        .map_err(|e| format!("the text could not be placed: {e}"))?;
    // What it says, so that the row in the window listing it can show the words
    // rather than "a stamp".
    let _ = annotation.set_contents(line);
    annotation
        .objects_mut()
        .add_text_object(object)
        .map_err(|e| format!("the text was refused: {e}"))?;
    Ok(())
}

/// The box the signature occupies, flipped into the page's own space.
///
/// It is padded by a nib on every side, because a stroke is drawn *centred* on
/// its path: a signature whose bounds are exactly its strokes has half a nib
/// of itself outside the rectangle, and a viewer that clips an annotation to
/// its `/Rect` — which is what the specification asks for — shaves the edge of
/// somebody's name.
fn box_of(at: &Rect, width: f64, space: &crate::markup::Space) -> PdfRect {
    let pad = (at.height.max(1.0) * NIB as f64).max(THINNEST as f64);
    space.up(&Rect {
        left: at.left - pad,
        top: at.top - pad,
        width: width + pad * 2.0,
        height: at.height + pad * 2.0,
    })
}
