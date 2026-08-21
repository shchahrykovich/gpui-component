//! What a find bar over a rendered document is given, and what comes back.
//!
//! The count and the place of the current occurrence are worked out while the
//! document is painted, so these tests draw a real frame rather than calling a
//! function.

use crate::text::{TextSearch, TextSearchResults, TextView};
use crate::{ActiveTheme as _, Root};
use gpui::{
    AppContext as _, Context, IntoElement, ParentElement as _, Render, Styled as _, TestAppContext,
    VisualTestContext, Window, div, px,
};

const SOURCE: &str = "\
# The cat

A **cat** sits on the mat.

- Cat food
- a dog
";

struct SearchedDocument {
    search: TextSearch,
}

impl Render for SearchedDocument {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(600.))
            .child(TextView::markdown("doc", SOURCE).search(self.search.clone()))
    }
}

fn draw(search: TextSearch, cx: &mut TestAppContext) -> &mut VisualTestContext {
    cx.update(crate::init);
    let (_, cx) = cx.add_window_view(|window, cx| {
        let doc = cx.new(|_| SearchedDocument { search });
        Root::new(doc, window, cx)
    });
    let cx: &mut VisualTestContext = cx;
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx
}

/// Every occurrence is counted, whatever block it fell in, and the case of a
/// letter does not hide one.
#[gpui::test]
fn a_document_reports_what_it_holds(cx: &mut TestAppContext) {
    let results = TextSearchResults::default();
    let search = TextSearch::new("cat", gpui::yellow())
        .current(1)
        .results(results.clone());
    let cx = draw(search, cx);

    let found = results.get();
    assert_eq!(
        found.count, 3,
        "the heading, the bold word in the paragraph, and the list item"
    );
    assert!(
        found.current.is_some(),
        "the second occurrence is on screen, so the find bar can scroll to it"
    );

    // A second frame counts the same document again, not twice over.
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert_eq!(results.get().count, 3);
}

/// A document taller than the window still reports what is below the fold.
///
/// The count is what the find bar says, and "3 of 4" from a reader who can see
/// only two of them is the whole point of counting. Blocks that are scrolled
/// out of view are still laid out and still painted — the viewport only clips
/// the pixels — which is what makes counting them possible at all.
#[gpui::test]
fn a_word_below_the_fold_is_counted_too(cx: &mut TestAppContext) {
    let results = TextSearchResults::default();
    let search = TextSearch::new("cat", gpui::yellow()).results(results.clone());

    cx.update(crate::init);
    let (_, cx) = cx.add_window_view(|window, cx| {
        let doc = cx.new(|_| LongDocument { search });
        Root::new(doc, window, cx)
    });
    let cx: &mut VisualTestContext = cx;
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });

    assert_eq!(
        results.get().count,
        LONG_PARAGRAPHS,
        "every paragraph holds the word once, and most of them are off screen"
    );
}

/// A word the document does not hold is answered, not refused.
#[gpui::test]
fn a_word_that_is_not_there_is_nought_of_nought(cx: &mut TestAppContext) {
    let results = TextSearchResults::default();
    let search = TextSearch::new("zebra", gpui::yellow()).results(results.clone());
    let _ = draw(search, cx);

    assert_eq!(results.get().count, 0);
    assert_eq!(results.get().current, None);
}

/// An occurrence the reader is not on is still painted; only the one they are
/// on is reported back, and an index past the end names none of them.
#[gpui::test]
fn an_index_past_the_end_leaves_no_current_occurrence(cx: &mut TestAppContext) {
    let results = TextSearchResults::default();
    let search = TextSearch::new("cat", gpui::yellow())
        .current(9)
        .results(results.clone());
    let _ = draw(search, cx);

    let found = results.get();
    assert_eq!(found.count, 3);
    assert_eq!(found.current, None);
}

/// The colours are the caller's, and a document nobody is searching is drawn
/// exactly as it was.
#[gpui::test]
fn a_document_with_no_search_reports_nothing(cx: &mut TestAppContext) {
    let results = TextSearchResults::default();
    cx.update(crate::init);
    let (_, cx) = cx.add_window_view(|window, cx| {
        let doc = cx.new(|_| PlainDocument);
        Root::new(doc, window, cx)
    });
    let cx: &mut VisualTestContext = cx;
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });

    assert_eq!(results.get(), Default::default());
    let _ = cx;
}

struct PlainDocument;

impl Render for PlainDocument {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _ = cx.theme();
        div().w(px(600.)).child(TextView::markdown("plain", SOURCE))
    }
}

/// Far more paragraphs than any test window can show at once.
const LONG_PARAGRAPHS: usize = 200;

struct LongDocument {
    search: TextSearch,
}

impl Render for LongDocument {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let source = "One cat per paragraph.\n\n".repeat(LONG_PARAGRAPHS);
        div()
            .w(px(600.))
            .h(px(300.))
            .child(TextView::markdown("long", source).search(self.search.clone()))
    }
}
