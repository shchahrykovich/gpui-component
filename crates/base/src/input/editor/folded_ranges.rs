//! Folds a caller chooses, rather than ones the reader clicks.
//!
//! Ordinary folding here is a reader's control: the syntax tree offers a
//! candidate on the line a block opens, a chevron appears in the gutter, and
//! clicking it folds that block. Everything about that is driven by the
//! grammar, and the candidates are built again from the syntax tree on every
//! reparse.
//!
//! This is the other kind. The usual caller is a version-control integration
//! that already draws which lines changed — see [`super::GutterMark`] — and
//! wants to offer "show only what changed": fold away the stretches the last
//! commit already had. Those stretches are not blocks, the grammar has no
//! opinion about them, and they must not be thrown away the next time the file
//! is highlighted.
//!
//! So a pinned fold is kept beside the ones the reader chose. It needs no
//! candidate, it survives a reparse, and clearing it leaves the reader's own
//! folds exactly as they were.

use gpui::Context;

use super::{EditorMode, FoldRange, InputBaseState};

impl InputBaseState<EditorMode> {
    /// Replace the folds this caller has pinned.
    ///
    /// Each range is folded whole, and the fold behaves like any other: the
    /// **first and last line of a range stay visible** and only what lies
    /// between them is hidden. A caller that wants lines `a` through `b` gone
    /// therefore asks for `a - 1 .. b + 1`, and a range with fewer than three
    /// lines in it hides nothing.
    ///
    /// Ranges are addressed by buffer line, so they do not follow edits, the
    /// same way [`super::GutterMark`] does not — and for the same reason: they
    /// come from an outside comparison that has to be run again anyway.
    pub fn set_folded_ranges(&mut self, ranges: Vec<FoldRange>, cx: &mut Context<Self>) {
        self.display_map.set_pinned_folds(ranges);
        cx.notify();
    }

    /// Unfold everything this caller pinned, leaving the reader's own folds.
    pub fn clear_folded_ranges(&mut self, cx: &mut Context<Self>) {
        self.set_folded_ranges(Vec::new(), cx);
    }
}
