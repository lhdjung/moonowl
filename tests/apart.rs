//! lopdf's work done in a process of its own (`objects::serve`), by the
//! binary cargo built, as the app does it.

use lopdf::dictionary;
use moonowl::{arrange, fixture, objects, render};

/// Every test here sends lopdf's work to the binary.
fn apart(name: &str) -> std::path::PathBuf {
    objects::work_apart(env!("CARGO_BIN_EXE_Moonowl").into());
    let dir = std::env::temp_dir().join(format!("moonowl-apart-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory to write in");
    dir
}

#[test]
fn labels_follow_their_pages_through_the_other_process() {
    let path = apart("labels").join("links.pdf");
    std::fs::copy(fixture::links_pdf(), &path).unwrap();
    let path = path.to_string_lossy().into_owned();
    let rest = (2..=6).map(|page| arrange::Sheet {
        from: None,
        page,
        turn: 0,
    });
    let wrote = arrange::Edit::Arrange(rest.collect()).write(&path).unwrap();
    assert!(wrote.labels_kept);
    assert_eq!(
        render::open(&path).unwrap().labels(),
        ["ii", "iii", "1", "2", "3"]
    );
}

#[test]
fn a_signed_document_is_updated_through_the_other_process() {
    let path = apart("signed").join("signed.pdf");
    std::fs::copy(fixture::signed_pdf(), &path).unwrap();
    let path = path.to_string_lossy().into_owned();
    let before = std::fs::read(&path).unwrap();
    let details = objects::Details {
        title: "Terms".into(),
        ..Default::default()
    };
    objects::write_details(&path, &details).unwrap();
    assert!(std::fs::read(&path).unwrap().starts_with(&before));
    assert_eq!(render::open(&path).unwrap().title(), "Terms");
}

/// A picture whose pixels are over `objects::LIMIT`, which the app's own
/// process does not decode.
#[test]
fn a_large_picture_is_made_into_a_page_in_the_other_process() {
    let dir = apart("picture");
    let png = dir.join("large.png");
    image::RgbImage::from_pixel(2000, 1500, image::Rgb([0, 128, 0]))
        .save(&png)
        .unwrap();
    let to = dir.join("picture.pdf").to_string_lossy().into_owned();
    let png = arrange::Sheet {
        from: Some(png.to_string_lossy().into_owned()),
        page: 1,
        turn: 0,
    };
    arrange::assemble(None, &[png], arrange::Layout::One, &to).unwrap();
    assert_eq!(render::open(&to).unwrap().pages(), 1);
    let drawn = render::open(&to)
        .unwrap()
        .render_owned(0, 40, 30, moonowl::layout::View::WHOLE)
        .unwrap();
    let middle = &drawn.bgra[(15 * 40 + 20) * 4..][..3];
    assert!(middle[1] > 100 && middle[0] < 30, "green: {middle:?}");
}

/// Over `objects::LIMIT`, which the app's own process does not read.
#[test]
fn a_large_document_is_reworked_in_the_other_process() {
    apart("large");
    let mut document = lopdf::Document::with_version("1.7");
    // Bytes that do not compress, so that the file stays large.
    let mut state = 1u64;
    let noise: Vec<u8> = (0..objects::LIMIT + 1024 * 1024)
        .map(|_| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            (state >> 56) as u8
        })
        .collect();
    let tree = document.new_object_id();
    let content = document.add_object(lopdf::Stream::new(dictionary! {}, noise));
    let page = document.add_object(dictionary! {
        "Type" => "Page", "Parent" => tree, "Contents" => content,
    });
    let pages = dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 };
    document
        .objects
        .insert(tree, lopdf::Object::Dictionary(pages));
    let catalog = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => tree });
    document.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).unwrap();
    assert!(bytes.len() > objects::LIMIT);
    let work = objects::Work {
        details: Some(objects::Details {
            title: "Large".into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    let (written, _) = objects::rework(bytes, &work).expect("reworked");
    let read = lopdf::Document::load_mem(&written).unwrap();
    let info = read
        .trailer
        .get(b"Info")
        .and_then(|info| read.dereference(info))
        .unwrap()
        .1;
    let title = info.as_dict().unwrap().get(b"Title").unwrap();
    assert_eq!(lopdf::decode_text_string(title).unwrap(), "Large");
}

/// A file the document carries is written by the other process, which reads
/// the document from the disk, and only under the name pdfium gave it.
#[test]
fn an_attachment_is_saved_by_the_other_process() {
    let dir = apart("attachment");
    let to = dir.join("data.csv").to_string_lossy().into_owned();
    let from = fixture::attached_pdf();
    assert!(!objects::save_attachment(&from, None, 0, "notes.txt", &to));
    assert!(!std::path::Path::new(&to).exists());
    assert!(objects::save_attachment(&from, None, 0, "data.csv", &to));
    assert_eq!(std::fs::read_to_string(&to).unwrap(), fixture::ATTACHED_CSV);
}

/// A file whose key in the document's tree is not its name, as some
/// programs write it, is still saved by the other process, under the name
/// pdfium gives it.
#[test]
fn an_attachment_keyed_by_another_name_is_saved_by_the_other_process() {
    let dir = apart("attachment-keyed");
    let mut bytes = std::fs::read(fixture::attached_pdf()).unwrap();
    // The same lengths, so that the cross-reference table still holds.
    for (key, other) in [
        ("[(data.csv)", "[(item.001)"),
        ("R (notes.txt)", "R (item.0002)"),
    ] {
        let at = (bytes.windows(key.len()))
            .position(|window| window == key.as_bytes())
            .expect("the key is in the fixture");
        bytes[at..at + key.len()].copy_from_slice(other.as_bytes());
    }
    let from = dir.join("keyed.pdf");
    std::fs::write(&from, &bytes).unwrap();
    let from = from.to_string_lossy().into_owned();
    let opened = render::open(&from).unwrap();
    assert_eq!(opened.attachments(), ["data.csv", "notes.txt"]);
    let to = dir.join("data.csv").to_string_lossy().into_owned();
    assert!(objects::save_attachment(&from, None, 0, "data.csv", &to));
    assert_eq!(std::fs::read_to_string(&to).unwrap(), fixture::ATTACHED_CSV);
}

/// **A compressed attachment and a large one** come out whole, through the
/// other process and through pdfium alike. The fixture's files are written
/// again compressed, with a third of 24MB beside them.
#[test]
fn a_compressed_and_a_large_attachment_are_saved_whole() {
    use lopdf::{Object, Stream};
    let dir = apart("attachments-large");
    let big: Vec<u8> = (0..24usize << 20).map(|at| (at * 7 % 251) as u8).collect();
    let mut document = lopdf::Document::load(fixture::attached_pdf()).unwrap();
    let mut stream = Stream::new(dictionary! { "Type" => "EmbeddedFile" }, big.clone());
    stream.compress().unwrap();
    let stream = document.add_object(stream);
    let spec = document.add_object(dictionary! {
        "Type" => "Filespec",
        "F" => Object::string_literal("big.bin"),
        "EF" => dictionary! { "F" => stream },
    });
    let names = document.catalog().unwrap().get(b"Names").unwrap().clone();
    let names = document
        .dereference(&names)
        .unwrap()
        .1
        .as_dict()
        .unwrap()
        .clone();
    let mut files = names
        .get(b"EmbeddedFiles")
        .unwrap()
        .as_dict()
        .unwrap()
        .clone();
    let list = files.get_mut(b"Names").unwrap().as_array_mut().unwrap();
    list.push(Object::string_literal("big.bin"));
    list.push(spec.into());
    let mut names = names;
    names.set("EmbeddedFiles", files);
    document.catalog_mut().unwrap().set("Names", names);
    document.compress();
    let from = dir.join("attached.pdf");
    document.save(&from).unwrap();
    let from = from.to_string_lossy().into_owned();
    assert!(
        std::fs::metadata(&from).unwrap().len() < 4 << 20,
        "the files in it are compressed"
    );

    let opened = render::open(&from).unwrap();
    assert_eq!(opened.attachments(), ["data.csv", "notes.txt", "big.bin"]);
    for (index, name, body) in [
        (0, "data.csv", fixture::ATTACHED_CSV.as_bytes()),
        (2, "big.bin", &big[..]),
    ] {
        assert!(
            opened.attachment(index).unwrap() == body,
            "{name} through pdfium"
        );
        let to = dir.join(name).to_string_lossy().into_owned();
        assert!(objects::save_attachment(&from, None, index, name, &to));
        assert!(std::fs::read(&to).unwrap() == body, "{name} apart");
    }
}
