//! **The search's reading never waits for a page being drawn**:
//! `try_text_of` reads a page's text while nobody holds pdfium, and answers
//! nothing at once while somebody does. Its own file, so that no other test
//! takes pdfium while it runs.

use moonowl::layout::View;
use moonowl::render;

#[test]
fn text_is_not_waited_for_while_the_renderer_holds_pdfium() {
    let document = render::open(&moonowl::fixture::prose_pdf()).expect("opens");
    assert!(
        document.try_text_of(0).is_some(),
        "the text is read while nobody holds pdfium"
    );
    // A page being drawn holds pdfium for as long as its pixels are lent.
    let mut read = None;
    document
        .render(0, 10, 10, View::WHOLE, &mut |_| {
            read = Some(document.try_text_of(0).is_some());
        })
        .expect("drawn");
    assert_eq!(read, Some(false), "and is not waited for while it is held");
}
