//! Which link the pointer is on, as a caller of `TextView::on_link_hover` is
//! told it.
//!
//! The answer is worked out from mouse moves over a painted document, so these
//! tests draw a real frame and move a simulated pointer over it.

use std::sync::{Arc, Mutex};

use gpui::{
    AppContext as _, Bounds, Context, Entity, IntoElement, Modifiers, ParentElement as _, Pixels,
    Point, Render, ScrollDelta, ScrollWheelEvent, SharedString, Styled as _, TestAppContext,
    VisualTestContext, Window, div, point, px,
};

use crate::text::{LinkHover, TextView, TextViewState};

/// Every call the handler received, in order.
type Calls = Arc<Mutex<Vec<Option<LinkHover>>>>;

struct Document {
    state: Entity<TextViewState>,
    width: Pixels,
    calls: Calls,
}

impl Render for Document {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let calls = self.calls.clone();
        div().w(self.width).child(
            TextView::new(&self.state).on_link_hover(move |hover, _, _| {
                calls.lock().unwrap().push(hover.cloned());
            }),
        )
    }
}

fn draw<'a>(
    source: &'static str,
    width: Pixels,
    cx: &'a mut TestAppContext,
) -> (Calls, &'a mut VisualTestContext) {
    cx.update(crate::init);
    let calls = Calls::default();
    let (_, cx) = cx.add_window_view({
        let calls = calls.clone();
        move |_, cx| Document {
            state: cx.new(|cx| TextViewState::markdown(source, cx)),
            width,
            calls,
        }
    });
    let cx: &mut VisualTestContext = cx;
    cx.run_until_parked();
    redraw(cx);
    (calls, cx)
}

fn redraw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
}

fn move_to(position: Point<Pixels>, cx: &mut VisualTestContext) {
    cx.simulate_mouse_move(position, None, Modifiers::default());
    redraw(cx);
}

/// What the handler was last told: the address, or `None` for "no link".
fn last_url(calls: &Calls) -> Option<SharedString> {
    calls
        .lock()
        .unwrap()
        .last()
        .cloned()
        .flatten()
        .map(|hover| hover.url)
}

/// Where the handler was last told the link was drawn.
fn last_bounds(calls: &Calls) -> Bounds<Pixels> {
    calls
        .lock()
        .unwrap()
        .last()
        .cloned()
        .flatten()
        .map(|hover| hover.bounds)
        .expect("a link is under the pointer")
}

#[gpui::test]
fn a_link_says_where_it_points(cx: &mut TestAppContext) {
    let (calls, cx) = draw("[example](https://example.com/page)", px(400.), cx);

    move_to(point(px(10.), px(10.)), cx);

    assert_eq!(
        last_url(&calls).as_deref(),
        Some("https://example.com/page"),
        "at once, with no delay: a status line is read in passing"
    );
}

#[gpui::test]
fn plain_text_says_nothing(cx: &mut TestAppContext) {
    let (calls, cx) = draw(
        "plain words and then [a link](https://example.com)",
        px(400.),
        cx,
    );

    move_to(point(px(10.), px(10.)), cx);

    assert!(calls.lock().unwrap().is_empty(), "no link, so no call");
}

/// The caller redraws on every call, so moving along one link must not repeat
/// what it has already been told.
#[gpui::test]
fn moving_along_a_link_tells_the_caller_once(cx: &mut TestAppContext) {
    let (calls, cx) = draw("[example](https://example.com)", px(400.), cx);

    move_to(point(px(8.), px(10.)), cx);
    move_to(point(px(12.), px(10.)), cx);
    move_to(point(px(16.), px(10.)), cx);

    assert_eq!(calls.lock().unwrap().len(), 1);
}

/// The pointer leaves the link for the words beside it, in the same run of
/// text, and for the empty space below the document, which no run covers.
#[gpui::test]
fn leaving_the_link_says_so(cx: &mut TestAppContext) {
    let places: [(&str, fn(Bounds<Pixels>) -> Point<Pixels>); 2] = [
        ("words beside it", |bounds| {
            point(bounds.right() + px(20.), bounds.center().y)
        }),
        ("space below it", |bounds| {
            point(bounds.left(), bounds.bottom() + px(200.))
        }),
    ];
    for (name, away) in places {
        let (calls, cx) = draw(
            "[example](https://example.com) and then plain words",
            px(400.),
            cx,
        );
        move_to(point(px(10.), px(10.)), cx);
        let bounds = last_bounds(&calls);

        move_to(away(bounds), cx);

        assert_eq!(last_url(&calls), None, "moved to the {name}");
    }
}

/// Two links in one paragraph are drawn by one run of text, so the run has to
/// tell them apart rather than only whether the pointer is on a link at all.
/// These two touch, so the pointer goes from one straight onto the other with
/// no plain text between them.
#[gpui::test]
fn the_next_link_says_where_it_points(cx: &mut TestAppContext) {
    let (calls, cx) = draw(
        "[first](https://one.example)[second](https://two.example)",
        px(400.),
        cx,
    );
    move_to(point(px(10.), px(10.)), cx);
    assert_eq!(last_url(&calls).as_deref(), Some("https://one.example"));

    let first = last_bounds(&calls);
    move_to(point(first.right() + px(10.), first.center().y), cx);

    assert_eq!(last_url(&calls).as_deref(), Some("https://two.example"));
}

/// A link that wraps is reported as the piece of it under the pointer, not as
/// the box around every line of it, so a caller that keeps clear of the link
/// keeps clear of the part the reader is looking at.
#[gpui::test]
fn a_link_that_wraps_is_reported_a_line_at_a_time(cx: &mut TestAppContext) {
    let (calls, cx) = draw(
        "[a link whose words do not fit on one line](https://wrap.example)",
        px(120.),
        cx,
    );
    move_to(point(px(10.), px(10.)), cx);
    let first = last_bounds(&calls);

    move_to(point(px(10.), first.bottom() + first.size.height / 2.), cx);
    let second = last_bounds(&calls);

    assert_eq!(last_url(&calls).as_deref(), Some("https://wrap.example"));
    assert!(
        second.top() >= first.bottom(),
        "the second piece is the next line down: {first:?} then {second:?}"
    );
    assert_eq!(
        first.size.height, second.size.height,
        "each piece is one line tall"
    );
}

/// A scroll moves the link out from under the pointer.
#[gpui::test]
fn a_scroll_says_the_link_is_gone(cx: &mut TestAppContext) {
    let (calls, cx) = draw("[example](https://example.com)", px(400.), cx);
    move_to(point(px(10.), px(10.)), cx);
    assert!(last_url(&calls).is_some());

    cx.simulate_event(ScrollWheelEvent {
        position: point(px(10.), px(10.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(-40.))),
        ..Default::default()
    });

    assert_eq!(last_url(&calls), None);
}
