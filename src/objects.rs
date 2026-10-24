//! The document's own objects, read and written through lopdf, for what
//! pdfium cannot write: an incremental update, and a smaller file.
//!
//! pdfium does the editing and saves the whole document. What happens here
//! comes after, on pdfium's bytes, and every function falls back to those
//! bytes when lopdf cannot read them or the result does not check out:
//! lopdf is less forgiving of a broken file than pdfium is.

use lopdf::{Document, Object, ObjectId};

/// The largest document that is compacted. lopdf holds a document as a great many
/// small objects, and the memory they took stays with the process once
/// they are freed: a 14MB book with 40,000 objects left the process 240MB
/// larger, where a paper of a few megabytes costs a few. A larger document
/// is written as pdfium saved it. A signed document is read whatever its
/// size, since the other way to change it breaks its signature.
// ponytail: a size stands in for the number of objects, which is what
// costs; a child process would give the memory back if large books matter.
pub const LIMIT: usize = 10 * 1024 * 1024;

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
/// update, read back, is not the document pdfium wrote.
pub fn incremental(original: &[u8], written: &[u8]) -> Option<Vec<u8>> {
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
///
/// Answers `bytes` as they were for a document over [`LIMIT`], when lopdf
/// cannot read them, when the document is encrypted, or when [`smaller`]
/// says so.
pub fn compact(bytes: Vec<u8>) -> Vec<u8> {
    if bytes.len() > LIMIT {
        return bytes;
    }
    match Document::load_mem(&bytes) {
        Ok(document) => smaller(document, bytes),
        Err(_) => bytes,
    }
}

/// `document`, which was read from `bytes` and perhaps changed since,
/// written as [`compact`] writes it. Answers `bytes` instead when the result
/// is no smaller, or when it does not read back as the same document.
pub fn smaller(mut document: Document, bytes: Vec<u8>) -> Vec<u8> {
    if document.is_encrypted() {
        return bytes;
    }
    prune(&mut document);
    document.compress();
    let options = lopdf::SaveOptions::builder()
        .use_object_streams(true)
        .use_xref_streams(true)
        .build();
    let mut written = Vec::new();
    if document.save_with_options(&mut written, options).is_err() || written.len() >= bytes.len() {
        return bytes;
    }
    let Ok(read) = Document::load_mem(&written) else {
        return bytes;
    };
    let whole = (document.objects.iter())
        .filter(|(_, object)| !layout(object))
        .all(|(id, object)| read.objects.get(id).is_some_and(|got| same(got, object)));
    if whole {
        written
    } else {
        bytes
    }
}

/// Whether an object is part of how the file is laid out rather than of the
/// document: a stream of objects or of cross-references, which a save makes
/// anew.
fn layout(object: &Object) -> bool {
    matches!(object.type_name(), Ok(b"ObjStm" | b"XRef"))
}

/// Whether two versions of an object say the same thing. A string is the
/// same whether it is written in hexadecimal or not, and a stream is
/// compared by what it holds, not by how it is compressed.
fn same(one: &Object, other: &Object) -> bool {
    match (one, other) {
        (Object::String(one, _), Object::String(other, _)) => one == other,
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
