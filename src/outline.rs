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
                leaves(document, tree, 0, &mut |name, place| {
                    named.insert(name.to_vec(), place.clone());
                });
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

/// Every name and destination in a name tree.
fn leaves(
    document: &Document,
    node: &Dictionary,
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
            if let Some(kid) = dictionary(document, Some(kid)) {
                leaves(document, kid, depth + 1, each);
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
/// before ends the reading, so an outline that loops is read once.
fn read(
    document: &Document,
    first: Option<ObjectId>,
    seen: &mut HashSet<ObjectId>,
    depth: usize,
) -> Vec<Node> {
    let mut nodes = Vec::new();
    let mut next = first;
    while let Some(id) = next.filter(|id| depth < DEPTH && seen.insert(*id)) {
        let Ok(item) = document.get_dictionary(id) else {
            break;
        };
        let reference = |key: &[u8]| item.get(key).and_then(Object::as_reference).ok();
        let children = read(document, reference(b"First"), seen, depth + 1);
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
/// others goes when none of them is left. Sets `changed` if any heading goes.
fn kept(document: &Document, places: &Places, nodes: Vec<Node>, changed: &mut bool) -> Vec<Node> {
    let mut kept = Vec::new();
    for node in nodes {
        let had_children = !node.children.is_empty();
        let children = self::kept(document, places, node.children, changed);
        let leads = (document.get_dictionary(node.id))
            .map_or(Leads::Nowhere, |item| places.of_item(document, item));
        match leads {
            Leads::Gone => {
                *changed = true;
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

/// Take out of `document` every heading, link and named destination that
/// leads to a page no longer in it. Answers whether any heading was taken
/// out.
pub fn forget_gone(document: &mut Document) -> bool {
    let places = Places::of(document);
    let mut changed = false;
    let root = (document.catalog().ok())
        .and_then(|catalog| catalog.get(b"Outlines").and_then(Object::as_reference).ok());
    if let Some(root) = root {
        let first = (document.get_dictionary(root).ok())
            .and_then(|outlines| outlines.get(b"First").and_then(Object::as_reference).ok());
        let nodes = read(document, first, &mut HashSet::new(), 0);
        let nodes = kept(document, &places, nodes, &mut changed);
        if changed {
            let shown = link(document, root, &nodes);
            if let Ok(outlines) = document.get_dictionary_mut(root) {
                outlines.set("Count", shown);
            }
        }
    }
    // A link or a named destination keeps the page it leads to in the file,
    // so these go too, though they were never shown.
    forget_links(document, &places);
    forget_named(document, &places);
    changed
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

/// Every named destination that leads to a page no longer there, taken out
/// of the catalog's `/Dests` and out of the leaves of the `/Names` tree.
fn forget_named(document: &mut Document, places: &Places) {
    let gone: HashSet<Vec<u8>> = (places.named.iter())
        .filter(|(_, place)| places.of_place(document, place, 1) == Leads::Gone)
        .map(|(name, _)| name.clone())
        .collect();
    if gone.is_empty() {
        return;
    }
    // Every object that holds names, found first and then changed, since
    // changing one while walking the others cannot be done.
    let mut holders = Vec::new();
    let mut waiting = Vec::new();
    if let Ok(catalog) = document.catalog() {
        if let Ok(Object::Reference(dests)) = catalog.get(b"Dests") {
            holders.push(*dests);
        }
        let tree = dictionary(document, catalog.get(b"Names").ok())
            .and_then(|names| names.get(b"Dests").ok());
        match tree {
            Some(Object::Reference(tree)) => waiting.push((*tree, 0)),
            // A root written into `/Names` is cleaned below, and its kids
            // are objects of their own.
            Some(Object::Dictionary(root)) => {
                waiting.extend(kids(document, root).into_iter().map(|kid| (kid, 1)));
            }
            _ => {}
        }
    }
    while let Some((node, depth)) = waiting.pop() {
        holders.push(node);
        if let (Ok(held), true) = (document.get_dictionary(node), depth < DEPTH) {
            waiting.extend(kids(document, held).into_iter().map(|kid| (kid, depth + 1)));
        }
    }
    for holder in holders {
        if let Ok(node) = document.get_dictionary_mut(holder) {
            clean(node, &gone);
        }
    }
    // The catalog's `/Dests` and the root of the tree, where they are
    // written into the catalog or into `/Names` rather than held apart.
    let mut names_held = None;
    if let Ok(catalog) = document.catalog_mut() {
        if let Ok(Object::Dictionary(dests)) = catalog.get_mut(b"Dests") {
            clean(dests, &gone);
        }
        match catalog.get_mut(b"Names") {
            Ok(Object::Dictionary(names)) => {
                if let Ok(Object::Dictionary(root)) = names.get_mut(b"Dests") {
                    clean(root, &gone);
                }
            }
            Ok(Object::Reference(id)) => names_held = Some(*id),
            _ => {}
        }
    }
    if let Some(Ok(names)) = names_held.map(|id| document.get_dictionary_mut(id)) {
        if let Ok(Object::Dictionary(root)) = names.get_mut(b"Dests") {
            clean(root, &gone);
        }
    }
}

/// The kids of a node of a name tree, which are always objects of their own.
fn kids(document: &Document, node: &Dictionary) -> Vec<ObjectId> {
    (node.get(b"Kids").ok())
        .and_then(|kids| document.dereference(kids).ok())
        .and_then(|(_, kids)| kids.as_array().ok())
        .into_iter()
        .flatten()
        .filter_map(|kid| kid.as_reference().ok())
        .collect()
}

/// `node` without the names in `gone`: a node of a name tree, or a
/// dictionary of names such as the catalog's `/Dests`.
fn clean(node: &mut Dictionary, gone: &HashSet<Vec<u8>>) {
    if node.has(b"Kids") || node.has(b"Names") {
        if let Ok(Object::Array(names)) = node.get_mut(b"Names") {
            let pairs: Vec<Object> = (names.chunks(2))
                .filter(|pair| !matches!(pair.first(), Some(Object::String(name, _)) if gone.contains(name)))
                .flatten()
                .cloned()
                .collect();
            *names = pairs;
        }
    } else {
        let names: Vec<Vec<u8>> = node.iter().map(|(name, _)| name.clone()).collect();
        for name in names.iter().filter(|name| gone.contains(*name)) {
            node.remove(name);
        }
    }
}

/// One heading written into a new file.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Entry {
    pub title: String,
    pub depth: usize,
    /// The zero-based index of the page in the new file it leads to.
    pub page: usize,
    /// Where on that page, in the page's own space, or `None` for the page
    /// as a whole.
    pub spot: Option<crate::render::Pin>,
}

/// The headings of `headings` (read from a document by
/// `pdfium::read_outline`) whose pages are in a new file, where `place`
/// says which page of the new file each old page (one-based) became. A
/// heading whose own heading is not kept goes under the nearest one that is.
pub fn entries(
    headings: &[crate::render::Heading],
    place: impl Fn(usize) -> Option<usize>,
) -> Vec<Entry> {
    // The old depths of the headings kept above the one being read.
    let mut above: Vec<usize> = Vec::new();
    let mut entries = Vec::new();
    for heading in headings {
        while above.last().is_some_and(|&depth| depth >= heading.depth) {
            above.pop();
        }
        let Some(page) = heading.page.and_then(&place) else {
            continue;
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
        .filter(|entry| pages.contains_key(&(entry.page as u32 + 1)))
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
        let page = pages[&(entry.page as u32 + 1)];
        let mut item = Dictionary::new();
        item.set("Title", lopdf::text_string(&entry.title));
        item.set("Dest", place(page, entry.spot));
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
/// alone. Answers whether any link was written.
pub fn relink(document: &mut Document, links: &[Relink]) -> bool {
    let pages = document.get_pages();
    let mut written = false;
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
            written = true;
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
    fn named_destinations_written_in_place_are_taken_out_too() {
        let mut document = Document::with_version("1.7");
        let tree = document.new_object_id();
        let kept = document.add_object(dictionary! { "Type" => "Page", "Parent" => tree });
        // Taken out of the page tree, as pdfium leaves a removed page.
        let removed = document.add_object(dictionary! { "Type" => "Page", "Parent" => tree });
        let pages = dictionary! { "Type" => "Pages", "Kids" => vec![kept.into()], "Count" => 1 };
        document.objects.insert(tree, Object::Dictionary(pages));
        let to = |page: ObjectId| Object::Array(vec![page.into(), "Fit".into()]);
        let catalog = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => tree,
            "Dests" => dictionary! { "here" => to(kept), "there" => to(removed) },
            "Names" => dictionary! { "Dests" => dictionary! { "Names" => vec![
                Object::string_literal("a"), to(removed), Object::string_literal("b"), to(kept),
            ] } },
        });
        document.trailer.set("Root", catalog);
        forget_gone(&mut document);
        let catalog = document.catalog().unwrap();
        let dests = catalog.get(b"Dests").unwrap().as_dict().unwrap();
        assert!(dests.has(b"here") && !dests.has(b"there"));
        let names = catalog.get(b"Names").unwrap().as_dict().unwrap();
        let root = names.get(b"Dests").unwrap().as_dict().unwrap();
        let left = root.get(b"Names").unwrap().as_array().unwrap();
        assert_eq!(left.len(), 2);
        assert_eq!(left[0].as_str().unwrap(), b"b");
    }
}
