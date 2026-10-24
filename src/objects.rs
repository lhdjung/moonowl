//! The document's own objects, read and written through lopdf, for what
//! pdfium cannot write: an incremental update, a smaller file, and the
//! document's details.
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
/// larger one is written as pdfium saved it. A signed document is read
/// whatever its size, since the other way to change it breaks its
/// signature. See the module's comment for why the size matters.
pub const LIMIT: usize = 10 * 1024 * 1024;

/// The argument that makes the app's executable do one piece of lopdf work
/// and end. See [`serve`].
pub const APART: &str = "--lopdf-work";

/// The executable that does the work, once the app has named it. Until then
/// the work is done in this process.
static HELPER: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Do lopdf's work in a process of its own, by running `executable` with
/// [`APART`]. The app names itself at start; a test names the binary cargo
/// built.
pub fn work_apart(executable: PathBuf) {
    let _ = HELPER.set(executable);
}

/// What is done to a document through lopdf, written out so that another
/// process can do it. Everything here is optional, and is done in this
/// order.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Work {
    /// Each page keeps only the resources its content uses
    /// (`arrange::slim_pages`).
    pub slim: bool,
    /// Every page keeps the label it had before this edit
    /// (`labels::follow`). The number is how many pages the document had
    /// before it, which an `Insert` does not say for itself.
    pub follow: Option<(crate::arrange::Edit, usize)>,
    /// What led to removed pages is taken out (`outline::forget_gone`).
    pub forget: bool,
    /// These headings are written as the outline (`outline::write`).
    pub outline: Vec<crate::outline::Entry>,
    /// These links are pointed at their pages again (`outline::relink`).
    pub relink: Vec<crate::outline::Relink>,
    /// The pages from this one (one-based) on numbered as this label begins
    /// (`labels::number_from`).
    pub number: Option<(usize, String)>,
    /// The title, author and subject written into `/Info`.
    pub details: Option<Details>,
}

/// What the work found, beside the document it wrote.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Done {
    /// Whether the document carries a digital signature ([`signed`]).
    pub signed: bool,
    /// Whether any heading was taken out by `forget`.
    pub forgot: bool,
    /// Why the pages could not be numbered as asked, if they could not.
    pub refused: Option<String>,
}

impl Work {
    /// Do the work on `document`. Answers whether anything was changed.
    fn apply(&self, document: &mut Document, done: &mut Done) -> bool {
        done.signed = signed(document);
        if self.slim {
            crate::arrange::slim_pages(document);
        }
        let mut changed = false;
        if let Some((edit, had)) = &self.follow {
            let now = document.get_pages().len();
            let before = match edit {
                crate::arrange::Edit::Remove(pages) => now + pages.len(),
                crate::arrange::Edit::Insert(..) => *had,
                _ => now,
            };
            changed |= crate::labels::follow(document, &edit.moved(before, now));
        }
        if self.forget {
            done.forgot = crate::outline::forget_gone(document);
            changed |= done.forgot;
        }
        changed |= crate::outline::write(document, &self.outline);
        changed |= crate::outline::relink(document, &self.relink);
        if let Some((page, typed)) = &self.number {
            match crate::labels::number_from(document, page.saturating_sub(1), typed) {
                Ok(()) => changed = true,
                Err(refused) => done.refused = Some(refused),
            }
        }
        if let Some(details) = &self.details {
            set_details(document, details);
            changed = true;
        }
        changed
    }
}

/// One piece of work sent to the other process.
#[derive(Serialize, Deserialize)]
enum Ask {
    /// [`rework`], followed by the document.
    Rework(Box<Work>),
    /// [`incremental`], followed by the original and the written document.
    Incremental,
}

/// The other process's answer, followed by the document it wrote, if any.
#[derive(Serialize, Deserialize)]
enum Answer {
    Reworked(Done),
    Updated,
    Declined,
}

/// Write `bytes` with their length before them.
fn send(to: &mut impl Write, bytes: &[u8]) -> std::io::Result<()> {
    to.write_all(&(bytes.len() as u64).to_le_bytes())?;
    to.write_all(bytes)
}

/// Read what [`send`] wrote.
fn receive(from: &mut impl Read) -> std::io::Result<Vec<u8>> {
    let mut length = [0; 8];
    from.read_exact(&mut length)?;
    let mut bytes = vec![0; u64::from_le_bytes(length) as usize];
    from.read_exact(&mut bytes)?;
    Ok(bytes)
}

/// Run `ask` in the other process, with `documents`. Answers `None` when no
/// process was named or it could not be started, so that the work is done
/// here instead, and `Some(None)` when it started but did not finish.
fn apart(ask: &Ask, documents: &[&[u8]]) -> Option<Option<(Answer, Vec<u8>)>> {
    let mut command = std::process::Command::new(HELPER.get()?);
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
    let answer = std::thread::scope(|scope| {
        // Written beside the reading, since a pipe holds only so much.
        scope.spawn(move || {
            let asked = serde_json::to_vec(ask).unwrap_or_default();
            let _ = send(&mut input, &asked);
            for document in documents {
                let _ = send(&mut input, document);
            }
        });
        let answer: Answer = serde_json::from_slice(&receive(&mut output).ok()?).ok()?;
        let bytes = match answer {
            Answer::Declined => Vec::new(),
            _ => receive(&mut output).ok()?,
        };
        Some((answer, bytes))
    });
    let ended = child.wait().is_ok_and(|status| status.success());
    Some(answer.filter(|_| ended))
}

/// The other process's side: read one [`Ask`] from stdin, do it, and write
/// the [`Answer`] to stdout. The app's `main` calls this when it is run with
/// [`APART`], before anything else.
pub fn serve() -> std::io::Result<()> {
    let (mut input, mut output) = (std::io::stdin().lock(), std::io::stdout().lock());
    let ask: Ask = serde_json::from_slice(&receive(&mut input)?)?;
    let (answer, bytes) = match ask {
        Ask::Rework(work) => match rework_here(receive(&mut input)?, &work, usize::MAX) {
            Ok((bytes, done)) => (Answer::Reworked(done), bytes),
            Err(_) => (Answer::Declined, Vec::new()),
        },
        Ask::Incremental => {
            let original = receive(&mut input)?;
            match incremental_here(&original, &receive(&mut input)?) {
                Some(bytes) => (Answer::Updated, bytes),
                None => (Answer::Declined, Vec::new()),
            }
        }
    };
    send(&mut output, &serde_json::to_vec(&answer)?)?;
    if !matches!(answer, Answer::Declined) {
        send(&mut output, &bytes)?;
    }
    output.flush()
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
/// Answers `None` when lopdf cannot read either version, when the original
/// is encrypted or its cross-reference table had to be rebuilt, or when the
/// update, read back, is not the document pdfium wrote, or when the other
/// process that does it fails.
pub fn incremental(original: &[u8], written: &[u8]) -> Option<Vec<u8>> {
    match apart(&Ask::Incremental, &[original, written]) {
        Some(Some((Answer::Updated, bytes))) => Some(bytes),
        Some(_) => None,
        None => incremental_here(original, written),
    }
}

/// [`incremental`], done in this process.
fn incremental_here(original: &[u8], written: &[u8]) -> Option<Vec<u8>> {
    let old = Document::load_mem(original).ok()?;
    if old.is_encrypted() || old.xref_start == 0 {
        return None;
    }
    let new = Document::load_mem(written).ok()?;
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
pub fn compact(bytes: Vec<u8>) -> Vec<u8> {
    rework(bytes, &Work::default()).map_or_else(|bytes| bytes, |(bytes, _)| bytes)
}

/// The document in `bytes`, changed by `work` and then compacted. A
/// document the work did not change is written only if that makes it
/// smaller.
///
/// Answers `Err` with `bytes` as they were when lopdf cannot read them,
/// when the document is encrypted, when the result does not read back as
/// the same document, when an unchanged document comes out no smaller, or
/// when the other process that does the work fails. Done in this process,
/// a document over [`LIMIT`] is not read at all.
pub fn rework(bytes: Vec<u8>, work: &Work) -> Result<(Vec<u8>, Done), Vec<u8>> {
    match apart(&Ask::Rework(Box::new(work.clone())), &[&bytes]) {
        Some(Some((Answer::Reworked(done), written))) => Ok((written, done)),
        Some(_) => Err(bytes),
        None => rework_here(bytes, work, LIMIT),
    }
}

/// [`rework`], done in this process, on a document of at most `limit` bytes.
fn rework_here(bytes: Vec<u8>, work: &Work, limit: usize) -> Result<(Vec<u8>, Done), Vec<u8>> {
    if bytes.len() > limit {
        return Err(bytes);
    }
    match Document::load_mem(&bytes) {
        Ok(mut document) if !document.is_encrypted() => {
            let mut done = Done::default();
            let changed = work.apply(&mut document, &mut done);
            smaller(document, bytes, changed).map(|written| (written, done))
        }
        _ => Err(bytes),
    }
}

/// `document`, which was read from `bytes`, written as [`compact`] writes
/// it. Unless it was `changed`, it is written only if that is smaller.
fn smaller(mut document: Document, bytes: Vec<u8>, changed: bool) -> Result<Vec<u8>, Vec<u8>> {
    prune(&mut document);
    document.compress();
    let options = lopdf::SaveOptions::builder()
        .use_object_streams(true)
        .use_xref_streams(true)
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
    let whole = (document.objects.iter())
        .filter(|(_, object)| !layout(object))
        .all(|(id, object)| read.objects.get(id).is_some_and(|got| same(got, object)));
    if whole {
        Ok(written)
    } else {
        Err(bytes)
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
/// holds, not by how it is compressed.
fn same(one: &Object, other: &Object) -> bool {
    match (one, other) {
        (Object::String(one, _), Object::String(other, _)) => one == other,
        (Object::Integer(whole), Object::Real(real))
        | (Object::Real(real), Object::Integer(whole)) => *whole as f32 == *real,
        (Object::Array(one), Object::Array(other)) => {
            one.len() == other.len() && one.iter().zip(other).all(|(a, b)| same(a, b))
        }
        (Object::Dictionary(one), Object::Dictionary(other)) => same_entries(one, other, &[]),
        (Object::Stream(one), Object::Stream(other)) => {
            let encoding = [b"Filter".as_slice(), b"DecodeParms", b"Length"];
            (one.content == other.content && same_entries(&one.dict, &other.dict, &[]))
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
    crate::markup::rewrite(path, path, |bytes| match rework(bytes, &work) {
        Ok((bytes, done)) => Ok(crate::markup::Saved {
            bytes,
            signed: done.signed,
        }),
        Err(_) => Err(DETAILS_NOT_WRITTEN.into()),
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
            let update = incremental(&original, &written).expect("an update");
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
}
