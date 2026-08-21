//! Marks painted in the gutter, beside the line numbers.
//!
//! A gutter mark says something about a whole buffer row without touching the
//! text: the usual caller is a version-control integration drawing which lines
//! were added, rewritten or taken out, the way every editor with a Git gutter
//! does it.
//!
//! Marks are independent of [`super::TextDecoration`], which styles a range of
//! *text*. A decoration cannot reach the gutter, and a mark cannot change how a
//! character looks.

use std::ops::Range;

use gpui::{Context, Hsla};

use super::{EditorMode, InputBaseState};

/// How one mark is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GutterMarkShape {
    /// A bar covering every row in the range, full row height.
    Bar,
    /// A short bar on the boundary **above** `rows.start`, for something that
    /// is no longer in the buffer and so has no row of its own.
    Boundary,
}

/// One mark in the gutter.
#[derive(Debug, Clone, PartialEq)]
pub struct GutterMark {
    /// Buffer rows the mark covers, counted from zero, end exclusive. A
    /// [`GutterMarkShape::Boundary`] mark uses `start` only, and its range may
    /// be empty.
    pub rows: Range<usize>,
    pub color: Hsla,
    pub shape: GutterMarkShape,
}

impl GutterMark {
    /// A bar over `rows`.
    pub fn bar(rows: Range<usize>, color: Hsla) -> Self {
        Self {
            rows,
            color,
            shape: GutterMarkShape::Bar,
        }
    }

    /// A short mark on the boundary above `row`.
    pub fn boundary(row: usize, color: Hsla) -> Self {
        Self {
            rows: row..row,
            color,
            shape: GutterMarkShape::Boundary,
        }
    }

    /// Whether this mark has anything to draw between `first` and `last`.
    pub(crate) fn overlaps(&self, first: usize, last: usize) -> bool {
        match self.shape {
            GutterMarkShape::Bar => self.rows.start <= last && self.rows.end > first,
            GutterMarkShape::Boundary => self.rows.start >= first && self.rows.start <= last,
        }
    }

    /// Whether this mark applies to `row`.
    pub(crate) fn covers(&self, row: usize) -> bool {
        match self.shape {
            GutterMarkShape::Bar => self.rows.contains(&row),
            GutterMarkShape::Boundary => self.rows.start == row,
        }
    }
}

impl InputBaseState<EditorMode> {
    /// Replace every gutter mark.
    ///
    /// Marks are addressed by buffer row, so they do **not** follow edits the
    /// way a [`super::TextDecoration`] does: a caller that inserts a line is
    /// expected to work out the rows again. That suits the case they exist
    /// for, where the rows come from an outside comparison that has to be run
    /// again anyway.
    pub fn set_gutter_marks(&mut self, marks: Vec<GutterMark>, cx: &mut Context<Self>) {
        if self.extras.gutter_marks == marks {
            return;
        }
        self.extras.gutter_marks = marks;
        cx.notify();
    }

    /// Remove every gutter mark.
    pub fn clear_gutter_marks(&mut self, cx: &mut Context<Self>) {
        self.set_gutter_marks(Vec::new(), cx);
    }

    /// The marks currently set.
    pub fn gutter_marks(&self) -> &[GutterMark] {
        &self.extras.gutter_marks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bar_covers_its_rows_and_a_boundary_covers_one() {
        let bar = GutterMark::bar(2..5, gpui::green());
        assert!(!bar.covers(1));
        assert!(bar.covers(2));
        assert!(bar.covers(4));
        assert!(!bar.covers(5));

        let boundary = GutterMark::boundary(3, gpui::red());
        assert!(boundary.covers(3));
        assert!(!boundary.covers(2));
        assert!(!boundary.covers(4));
    }
}
