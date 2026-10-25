//! The document's outline, which the sidebar shows as its contents, kept
//! true when pages change. pdfium reads an outline but cannot write one, so
//! this works on the file's own objects through lopdf, after pdfium has
//! saved (see `objects::rework`).
//!
//! Two things are done here. After pages are removed, every heading, link
//! and named destination that leads to one of them is taken out. And a new
//! file made of some pages, or of several files, is given the headings of
//! the pages it holds.

use lopdf::{Dictionary, Document, Object, ObjectId};
use std::collections::{HashMap, HashSet};

/// How deep an outline is read. The specification sets no limit, and no
/// real document goes past a handful of levels.
const DEPTH: usize = 32;

/// Where something in the document leads.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Leads {
    /// To a page of the document, or somewhere this does not judge: another
    /// file, a web page, or a named destination the document does not have.
    Somewhere,
    /// To a page that is no longer in the document.
    Gone,
    /// Nowhere of its own: a heading that only groups the headings under it.
    Nowhere,
}

/// What the document needs to say where a destination leads: its pages, and
/// its named destinations.
struct Places {
    pages: HashSet<ObjectId>,
    named: HashMap<Vec<u8>, Object>,
}

impl Places {
    fn of(document: &Document) -> Places {
        let mut named = HashMap::new();
        if let Ok(catalog) = document.catalog() {
            if let Some(dests) = dictionary(document, catalog.get(b"Dests").ok()) {
                for (name, place) in dests.iter() {
                    named.insert(name.clone(), place.clone());
                }
            }
            let names = dictionary(document, catalog.get(b"Names").ok());
            if let Some(tree) =
                names.and_then(|names| dictionary(document, names.get(b"Dests").ok()))
            {
                leaves(
                    document,
                    tree,
                    &mut HashSet::new(),
                    0,
                    &mut |name, place| {
                        named.insert(name.to_vec(), place.clone());
                    },
                );
            }
        }
        Places {
            pages: document.get_pages().into_values().collect(),
            named,
        }
    }

    /// Where a destination leads: an explicit one, a named one, or the
    /// dictionary a named one may be written as.
    fn of_place(&self, document: &Document, place: &Object, hops: usize) -> Leads {
        let place = document
            .dereference(place)
            .map_or(place, |(_, place)| place);
        match place {
            Object::Array(place) => match place.first() {
                Some(Object::Reference(page)) if self.pages.contains(page) => Leads::Somewhere,
                // A page taken out of the tree is still in the file until it
                // is compacted, and a page object is what pdfium leaves.
                Some(Object::Reference(page)) => match document.get_object(*page) {
                    Ok(object) if object.type_name().is_ok_and(|name| name != b"Page") => {
                        Leads::Somewhere
                    }
                    _ => Leads::Gone,
                },
                Some(Object::Null) => Leads::Gone,
                _ => Leads::Somewhere,
            },
            Object::Name(name) | Object::String(name, _) if hops < 2 => {
                match self.named.get(name.as_slice()) {
                    Some(place) => self.of_place(document, place, hops + 1),
                    None => Leads::Somewhere,
                }
            }
            Object::Dictionary(place) if hops < 2 => match place.get(b"D") {
                Ok(place) => self.of_place(document, place, hops + 1),
                Err(_) => Leads::Somewhere,
            },
            _ => Leads::Somewhere,
        }
    }

    /// Where a heading or a link leads: its `/Dest`, or the destination of
    /// a GoTo action.
    fn of_item(&self, document: &Document, item: &Dictionary) -> Leads {
        if let Ok(place) = item.get(b"Dest") {
            return self.of_place(document, place, 0);
        }
        let Some(action) = dictionary(document, item.get(b"A").ok()) else {
            return Leads::Nowhere;
        };
        let goes = action.get(b"S").and_then(Object::as_name);
        match (goes, action.get(b"D")) {
            (Ok(b"GoTo"), Ok(place)) => self.of_place(document, place, 0),
            (Ok(_), _) => Leads::Somewhere,
            _ => Leads::Nowhere,
        }
    }
}

/// The dictionary `object` is, or refers to.
fn dictionary<'a>(document: &'a Document, object: Option<&'a Object>) -> Option<&'a Dictionary> {
    document.dereference(object?).ok()?.1.as_dict().ok()
}

/// Every name and destination in a name tree. A node seen before is not
/// read again, so a tree that lists a node twice is read once.
fn leaves(
    document: &Document,
    node: &Dictionary,
    seen: &mut HashSet<ObjectId>,
    depth: usize,
    each: &mut dyn FnMut(&[u8], &Object),
) {
    if depth > DEPTH {
        return;
    }
    if let Some(Ok(names)) = (node.get(b"Names").ok()).map(|names| {
        document
            .dereference(names)
            .and_then(|(_, names)| names.as_array())
    }) {
        for pair in names.chunks(2) {
            if let [Object::String(name, _), place] = pair {
                each(name, place);
            }
        }
    }
    if let Some(Ok(kids)) = (node.get(b"Kids").ok()).map(|kids| {
        document
            .dereference(kids)
            .and_then(|(_, kids)| kids.as_array())
    }) {
        for kid in kids {
            if kid.as_reference().is_ok_and(|id| !seen.insert(id)) {
                continue;
            }
            if let Some(kid) = dictionary(document, Some(kid)) {
                leaves(document, kid, seen, depth + 1, each);
            }
        }
    }
}

/// A heading of the outline, and the headings under it.
struct Node {
    id: ObjectId,
    /// Whether its headings are shown, which a positive `/Count` says.
    open: bool,
    children: Vec<Node>,
}

/// The headings from `first` on, and everything under them. A heading seen
/// before ends the reading, so an outline that loops is read once. Sets
/// `cut` if headings lie deeper than [`DEPTH`] and were not read.
fn read(
    document: &Document,
    first: Option<ObjectId>,
    seen: &mut HashSet<ObjectId>,
    depth: usize,
    cut: &mut bool,
) -> Vec<Node> {
    let mut nodes = Vec::new();
    let mut next = first;
    if depth >= DEPTH {
        *cut |= next.is_some();
        return nodes;
    }
    while let Some(id) = next.filter(|id| seen.insert(*id)) {
        let Ok(item) = document.get_dictionary(id) else {
            break;
        };
        let reference = |key: &[u8]| item.get(key).and_then(Object::as_reference).ok();
        let children = read(document, reference(b"First"), seen, depth + 1, cut);
        let open = item
            .get(b"Count")
            .and_then(Object::as_i64)
            .map_or(true, |count| count > 0);
        nodes.push(Node { id, open, children });
        next = reference(b"Next");
    }
    nodes
}

/// `nodes` without the headings that lead to a page no longer there; each
/// such heading's own headings take its place. A heading that only grouped
/// others goes when none of them is left. Sets `changed` if any heading
/// goes, and counts in `gone` the headings that led to a page no longer
/// there.
fn kept(
    document: &Document,
    places: &Places,
    nodes: Vec<Node>,
    changed: &mut bool,
    gone: &mut usize,
) -> Vec<Node> {
    let mut kept = Vec::new();
    for node in nodes {
        let had_children = !node.children.is_empty();
        let children = self::kept(document, places, node.children, changed, gone);
        let leads = (document.get_dictionary(node.id))
            .map_or(Leads::Nowhere, |item| places.of_item(document, item));
        match leads {
            Leads::Gone => {
                *changed = true;
                *gone += 1;
                kept.extend(children);
            }
            Leads::Nowhere if had_children && children.is_empty() => *changed = true,
            _ => kept.push(Node { children, ..node }),
        }
    }
    kept
}

/// Write `nodes` under `parent` as the outline's links say it: each knows
/// its parent and neighbours, and the parent its first and last. Answers
/// how many headings are shown under `parent` when it is open.
fn link(document: &mut Document, parent: ObjectId, nodes: &[Node]) -> i64 {
    let mut shown = 0;
    for (at, node) in nodes.iter().enumerate() {
        let below = link(document, node.id, &node.children);
        shown += 1 + if node.open { below } else { 0 };
        let Ok(item) = document.get_dictionary_mut(node.id) else {
            continue;
        };
        item.set("Parent", parent);
        match at.checked_sub(1).map(|before| nodes[before].id) {
            Some(before) => item.set("Prev", before),
            None => {
                item.remove(b"Prev");
            }
        }
        match nodes.get(at + 1) {
            Some(after) => item.set("Next", after.id),
            None => {
                item.remove(b"Next");
            }
        }
        if node.children.is_empty() {
            item.remove(b"Count");
        } else {
            item.set("Count", if node.open { below } else { -below });
        }
    }
    if let Ok(item) = document.get_dictionary_mut(parent) {
        match (nodes.first(), nodes.last()) {
            (Some(first), Some(last)) => {
                item.set("First", first.id);
                item.set("Last", last.id);
            }
            _ => {
                item.remove(b"First");
                item.remove(b"Last");
            }
        }
    }
    shown
}

/// Take out of `document` every heading and link that leads to a page no
/// longer in it, and make every other destination that leads there lead
/// nowhere. Answers how many headings were taken out.
///
/// An outline deeper than [`DEPTH`] is left as it is, since writing it
/// again would lose the headings that were not read. Its headings that led
/// to a removed page lead nowhere.
pub fn forget_gone(document: &mut Document) -> usize {
    let places = Places::of(document);
    let mut gone = 0;
    let root = (document.catalog().ok())
        .and_then(|catalog| catalog.get(b"Outlines").and_then(Object::as_reference).ok());
    if let Some(root) = root {
        let first = (document.get_dictionary(root).ok())
            .and_then(|outlines| outlines.get(b"First").and_then(Object::as_reference).ok());
        let (mut changed, mut cut) = (false, false);
        let nodes = read(document, first, &mut HashSet::new(), 0, &mut cut);
        let nodes = kept(document, &places, nodes, &mut changed, &mut gone);
        if cut {
            gone = 0;
        } else if changed {
            let shown = link(document, root, &nodes);
            if let Ok(outlines) = document.get_dictionary_mut(root) {
                outlines.set("Count", shown);
            }
        }
    }
    // A link or a named destination keeps the page it leads to in the file,
    // so these go too, though they were never shown.
    forget_links(document, &places);
    forget_pages(document, &places);
    gone
}

/// Every link on a page that leads to a page no longer there, taken off it.
fn forget_links(document: &mut Document, places: &Places) {
    for page in places.pages.iter().copied() {
        let Ok(annotations) = document
            .get_dictionary(page)
            .and_then(|page| page.get(b"Annots"))
        else {
            continue;
        };
        let (held, list) = match document.dereference(annotations) {
            Ok((held, Object::Array(list))) => (held, list.clone()),
            _ => continue,
        };
        let gone = |annotation: &Object| {
            dictionary(document, Some(annotation)).is_some_and(|annotation| {
                annotation
                    .get(b"Subtype")
                    .and_then(Object::as_name)
                    .is_ok_and(|kind| kind == b"Link")
                    && places.of_item(document, annotation) == Leads::Gone
            })
        };
        let left: Vec<Object> = list
            .iter()
            .filter(|annotation| !gone(annotation))
            .cloned()
            .collect();
        if left.len() == list.len() {
            continue;
        }
        match held {
            Some(id) => {
                document.objects.insert(id, Object::Array(left));
            }
            None => {
                if let Ok(page) = document.get_dictionary_mut(page) {
                    page.set("Annots", left);
                }
            }
        }
    }
}

/// Every destination still in the file that leads to a page no longer in
/// the document, such as a named destination, made to lead nowhere: the
/// page it names is put back as null, so that the page can be dropped.
fn forget_pages(document: &mut Document, places: &Places) {
    let removed: HashSet<ObjectId> = (document.objects.iter())
        .filter(|(id, object)| {
            !places.pages.contains(id) && object.type_name().is_ok_and(|name| name == b"Page")
        })
        .map(|(id, _)| *id)
        .collect();
    if removed.is_empty() {
        return;
    }
    // A destination is an array that begins with its page.
    fn unlink(object: &mut Object, removed: &HashSet<ObjectId>) {
        let entries = match object {
            Object::Array(items) => {
                if matches!(items.first(), Some(Object::Reference(page)) if removed.contains(page))
                {
                    items[0] = Object::Null;
                }
                items.iter_mut().for_each(|item| unlink(item, removed));
                return;
            }
            Object::Dictionary(entries) => entries,
            Object::Stream(stream) => &mut stream.dict,
            _ => return,
        };
        entries
            .iter_mut()
            .for_each(|(_, value)| unlink(value, removed));
    }
    for object in document.objects.values_mut() {
        unlink(object, &removed);
    }
}

/// One heading written into a new file.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Entry {
    pub title: String,
    pub depth: usize,
    /// The zero-based index of the page in the new file it leads to, or
    /// `None` for a heading that only groups the headings under it.
    pub page: Option<usize>,
    /// Where on that page, in the page's own space, or `None` for the page
    /// as a whole.
    pub spot: Option<crate::render::Pin>,
}

/// The headings of `headings` (read from a document by
/// `pdfium::read_outline`) whose pages are in a new file, where `place`
/// says which page of the new file each old page (one-based) became. A
/// heading whose own heading is not kept goes under the nearest one that is.
/// A heading that leads nowhere and only groups others is kept while one
/// of the headings under it is.
pub fn entries(
    headings: &[crate::render::Heading],
    place: impl Fn(usize) -> Option<usize>,
) -> Vec<Entry> {
    // The old depths of the headings kept above the one being read.
    let mut above: Vec<usize> = Vec::new();
    let mut entries = Vec::new();
    for (at, heading) in headings.iter().enumerate() {
        while above.last().is_some_and(|&depth| depth >= heading.depth) {
            above.pop();
        }
        let page = match heading.page {
            Some(page) => match place(page) {
                Some(page) => Some(page),
                None => continue,
            },
            None => {
                let groups = (headings[at + 1..].iter())
                    .take_while(|under| under.depth > heading.depth)
                    .any(|under| under.page.and_then(&place).is_some());
                if !groups {
                    continue;
                }
                None
            }
        };
        entries.push(Entry {
            title: heading.title.clone(),
            depth: above.len(),
            page,
            spot: heading.spot,
        });
        above.push(heading.depth);
    }
    entries
}

/// Write `entries` into `document` as its outline, in place of any it has.
/// A heading with headings under it is written closed, which is how most
/// documents ship. Answers whether there was anything to write.
pub fn write(document: &mut Document, entries: &[Entry]) -> bool {
    let pages = document.get_pages();
    let entries: Vec<&Entry> = (entries.iter())
        .filter(|entry| (entry.page).is_none_or(|page| pages.contains_key(&(page as u32 + 1))))
        .collect();
    if entries.is_empty() {
        return false;
    }
    let ids: Vec<ObjectId> = entries.iter().map(|_| document.new_object_id()).collect();
    // Each heading's place in the tree, from the depths: the nodes under
    // the root, each holding its own.
    fn tree(entries: &[&Entry], ids: &[ObjectId], at: &mut usize, depth: usize) -> Vec<Node> {
        let mut nodes = Vec::new();
        while *at < entries.len() && entries[*at].depth >= depth {
            let id = ids[*at];
            *at += 1;
            let children = tree(entries, ids, at, depth + 1);
            nodes.push(Node {
                id,
                open: false,
                children,
            });
        }
        nodes
    }
    for (entry, id) in entries.iter().zip(&ids) {
        let mut item = Dictionary::new();
        item.set("Title", lopdf::text_string(&entry.title));
        if let Some(page) = entry.page {
            item.set("Dest", place(pages[&(page as u32 + 1)], entry.spot));
        }
        document.objects.insert(*id, Object::Dictionary(item));
    }
    let nodes = tree(&entries, &ids, &mut 0, 0);
    let root = document.add_object(lopdf::dictionary! { "Type" => "Outlines" });
    let shown = link(document, root, &nodes);
    if let Ok(outlines) = document.get_dictionary_mut(root) {
        outlines.set("Count", shown);
    }
    match document.catalog_mut() {
        Ok(catalog) => {
            catalog.set("Outlines", root);
            true
        }
        Err(_) => false,
    }
}

/// A destination: `page`, at `spot` on it, or the page as a whole.
fn place(page: ObjectId, spot: Option<crate::render::Pin>) -> Vec<Object> {
    match spot {
        Some(spot) => vec![
            page.into(),
            "XYZ".into(),
            spot.left
                .map_or(Object::Null, |left| Object::Real(left as f32)),
            Object::Real(spot.top as f32),
            Object::Null,
        ],
        None => vec![page.into(), "Fit".into()],
    }
}

/// A link on a page that was copied, to another page copied with it.
/// pdfium copies a link without its destination, so it is written again.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Relink {
    /// The page the link is on, zero-based, in the file being written.
    pub page: usize,
    /// Which of the page's annotations it is, which pdfium keeps in order.
    pub annotation: usize,
    /// The page it leads to, zero-based, in the file being written.
    pub to: usize,
    /// Where on that page, in the page's own space.
    pub spot: Option<crate::render::Pin>,
}

/// Point every link of `links` at its page again. A link that is not where
/// it should be (no annotation there, or one that is not a link) is left
/// alone. Answers how many links were written.
pub fn relink(document: &mut Document, links: &[Relink]) -> usize {
    let pages = document.get_pages();
    let mut written = 0;
    for link in links {
        let (Some(&page), Some(&to)) = (
            pages.get(&(link.page as u32 + 1)),
            pages.get(&(link.to as u32 + 1)),
        ) else {
            continue;
        };
        let Some(annotation) = annotation_mut(document, page, link.annotation) else {
            continue;
        };
        if annotation
            .get(b"Subtype")
            .and_then(Object::as_name)
            .is_ok_and(|kind| kind == b"Link")
        {
            annotation.remove(b"A");
            annotation.set("Dest", place(to, link.spot));
            written += 1;
        }
    }
    written
}

/// The annotation at `index` of `page`'s `/Annots`, wherever it is held: in
/// an object of its own, or written into the array, which is itself in the
/// page or in an object of its own.
fn annotation_mut(
    document: &mut Document,
    page: ObjectId,
    index: usize,
) -> Option<&mut Dictionary> {
    let annotations = document.get_dictionary(page).ok()?.get(b"Annots").ok()?;
    let (held, element) = match annotations {
        Object::Reference(array) => (
            Some(*array),
            document
                .get_object(*array)
                .ok()?
                .as_array()
                .ok()?
                .get(index)?,
        ),
        Object::Array(array) => (None, array.get(index)?),
        _ => return None,
    };
    if let Object::Reference(id) = element {
        let id = *id;
        return document.get_dictionary_mut(id).ok();
    }
    let array = match held {
        Some(array) => document.get_object_mut(array).ok()?,
        None => document
            .get_dictionary_mut(page)
            .ok()?
            .get_mut(b"Annots")
            .ok()?,
    };
    array
        .as_array_mut()
        .ok()?
        .get_mut(index)?
        .as_dict_mut()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    #[test]
    fn destinations_to_a_removed_page_lead_nowhere() {
        let mut document = Document::with_version("1.7");
        let tree = document.new_object_id();
        let kept = document.add_object(dictionary! { "Type" => "Page", "Parent" => tree });
        // Taken out of the page tree, as pdfium leaves a removed page.
        let removed = document.add_object(dictionary! { "Type" => "Page", "Parent" => tree });
        let pages = dictionary! { "Type" => "Pages", "Kids" => vec![kept.into()], "Count" => 1 };
        document.objects.insert(tree, Object::Dictionary(pages));
        let to = |page: ObjectId| Object::Array(vec![page.into(), "Fit".into()]);
        // A node of the name tree that lists itself as its own kid.
        let looped = document.new_object_id();
        let node = dictionary! {
            "Kids" => vec![looped.into(), looped.into()],
            "Names" => vec![Object::string_literal("c"), to(removed)],
        };
        document.objects.insert(looped, Object::Dictionary(node));
        let catalog = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => tree,
            "Dests" => dictionary! { "here" => to(kept), "there" => to(removed) },
            "Names" => dictionary! { "Dests" => dictionary! {
                "Names" => vec![
                    Object::string_literal("a"), to(removed), Object::string_literal("b"), to(kept),
                ],
                "Kids" => vec![looped.into()],
            } },
        });
        document.trailer.set("Root", catalog);
        forget_gone(&mut document);
        let refers = |object: &Object| format!("{object:?}").contains(&format!("{removed:?}"));
        let left = (document.objects.iter())
            .filter(|(id, object)| **id != removed && refers(object))
            .count();
        assert_eq!(left, 0, "nothing leads to the removed page");
        let catalog = document.catalog().unwrap();
        let dests = catalog.get(b"Dests").unwrap().as_dict().unwrap();
        assert_eq!(dests.get(b"here").unwrap(), &to(kept));
    }

    #[test]
    fn a_heading_that_only_groups_others_is_kept_with_them() {
        let heading = |title: &str, depth, page| crate::render::Heading {
            title: title.into(),
            depth,
            page,
            offset: 0.0,
            spot: None,
        };
        let headings = [
            heading("Part I", 0, None),
            heading("One", 1, Some(1)),
            heading("Part II", 0, None),
            heading("Two", 1, Some(2)),
        ];
        let entries = entries(&headings, |page| (page == 2).then_some(0));
        let kept: Vec<(&str, usize, Option<usize>)> = (entries.iter())
            .map(|entry| (entry.title.as_str(), entry.depth, entry.page))
            .collect();
        assert_eq!(kept, [("Part II", 0, None), ("Two", 1, Some(0))]);
    }
}
