//! Lines drawn in the editor that the buffer does not hold.
//!
//! A phantom block is a run of lines shown directly above one buffer row. It
//! takes the height of the rows it draws — everything below it moves down —
//! and it is not text: the caret cannot reach it, a selection never covers it,
//! and saving the buffer never writes it.
//!
//! The caller it exists for is a version-control integration showing the lines
//! a commit took out. Those lines are in no buffer, so there is no row to
//! colour and no range to decorate; the only honest place for them is above the
//! row they were taken from.
//!
//! Blocks are addressed by buffer row and do not follow edits, exactly as a
//! [`super::GutterMark`] does not. That suits the same case: the rows come from
//! a comparison made outside the editor, which has to be run again after an
//! edit anyway.

use gpui::{Context, Hsla, SharedString};

use super::{EditorMode, InputBaseState, InputExtras as _, InputModeKind};

/// A run of lines shown above a buffer row, which the buffer does not hold.
#[derive(Debug, Clone, PartialEq)]
pub struct PhantomLines {
    /// The buffer row this block sits above, counted from zero. The line count
    /// itself puts the block after the last line of the buffer, which is where
    /// lines taken from the end of a file belong.
    pub row: usize,
    /// The lines, in order. Each is drawn at one row's height.
    pub lines: Vec<SharedString>,
    /// The colour the lines are drawn in.
    pub color: Hsla,
    /// A wash painted across each of the block's rows, when there is one.
    pub background: Option<Hsla>,
    /// A bar drawn in the gutter beside the block's rows, when there is one.
    ///
    /// The block's rows wear no line number, so this is the only thing in the
    /// gutter that says what they are.
    pub mark: Option<Hsla>,
}

impl PhantomLines {
    /// A block of `lines` above `row`, drawn in `color`.
    pub fn new(row: usize, lines: Vec<SharedString>, color: Hsla) -> Self {
        Self {
            row,
            lines,
            color,
            background: None,
            mark: None,
        }
    }

    /// Paint `color` across each of the block's rows.
    pub fn background(mut self, color: Hsla) -> Self {
        self.background = Some(color);
        self
    }

    /// Draw a bar in `color` beside the block's rows, in the gutter.
    pub fn mark(mut self, color: Hsla) -> Self {
        self.mark = Some(color);
        self
    }

    /// How many rows the block takes.
    pub(crate) fn rows(&self) -> usize {
        self.lines.len()
    }
}

/// The rows of every block above buffer line `row` that does not sit on it.
///
/// `on_screen` answers whether the row a block sits above is drawn at all: a
/// block whose row a fold has hidden is hidden with it, and must not be counted
/// or every row below the fold would be offset by a block nobody can see.
pub(crate) fn rows_before(
    blocks: &[PhantomLines],
    row: usize,
    on_screen: impl Fn(usize) -> bool,
) -> usize {
    blocks
        .iter()
        .filter(|block| block.row < row && on_screen(block.row))
        .map(PhantomLines::rows)
        .sum()
}

/// The rows of the block sitting directly above buffer line `row`.
pub(crate) fn rows_at(
    blocks: &[PhantomLines],
    row: usize,
    on_screen: impl Fn(usize) -> bool,
) -> usize {
    match block_at(blocks, row, on_screen) {
        Some(block) => block.rows(),
        None => 0,
    }
}

/// The block sitting directly above buffer line `row`, if it is drawn.
pub(crate) fn block_at(
    blocks: &[PhantomLines],
    row: usize,
    on_screen: impl Fn(usize) -> bool,
) -> Option<&PhantomLines> {
    blocks
        .iter()
        .find(|block| block.row == row)
        .filter(|block| on_screen(block.row))
}

/// Every phantom row in the buffer.
pub(crate) fn rows_total(blocks: &[PhantomLines], on_screen: impl Fn(usize) -> bool) -> usize {
    blocks
        .iter()
        .filter(|block| on_screen(block.row))
        .map(PhantomLines::rows)
        .sum()
}

/// The display row drawn at `visual_row`, counting the phantom rows above it.
///
/// A visual row is what the reader is looking at: display rows and phantom rows
/// together, all of one height. A visual row that lands *inside* a block
/// answers with the display row the block sits above, so the frame that asks
/// what to draw at the top of the viewport is told about the block itself.
///
/// `blocks` must be sorted by row, which is what [`InputBaseState::set_phantom_lines`]
/// keeps them in.
pub(crate) fn display_row_at_visual_row(
    blocks: &[PhantomLines],
    visual_row: usize,
    display_row_of: impl Fn(usize) -> usize,
    on_screen: impl Fn(usize) -> bool,
    display_count: usize,
) -> usize {
    let last = display_count.saturating_sub(1);
    let mut phantom_above = 0;

    for block in blocks {
        if !on_screen(block.row) {
            continue;
        }
        let top = display_row_of(block.row) + phantom_above;
        if visual_row < top {
            break;
        }
        if visual_row < top + block.rows() {
            return display_row_of(block.row).min(last);
        }
        phantom_above += block.rows();
    }

    visual_row.saturating_sub(phantom_above).min(last)
}

impl InputBaseState<EditorMode> {
    /// Replace every phantom block.
    ///
    /// The blocks are sorted by row here, because everything that reads them
    /// walks them in order.
    pub fn set_phantom_lines(&mut self, mut blocks: Vec<PhantomLines>, cx: &mut Context<Self>) {
        blocks.sort_by_key(|block| block.row);
        if self.extras.phantom_lines == blocks {
            return;
        }
        self.extras.phantom_lines = blocks;
        cx.notify();
    }

    /// Remove every phantom block.
    pub fn clear_phantom_lines(&mut self, cx: &mut Context<Self>) {
        self.set_phantom_lines(Vec::new(), cx);
    }

    /// The blocks currently set, in row order.
    pub fn phantom_lines(&self) -> &[PhantomLines] {
        &self.extras.phantom_lines
    }
}

impl<M: InputModeKind> InputBaseState<M> {
    /// Whether the row a block sits above is drawn at all.
    ///
    /// A block past the last line of the buffer is always drawn: it is the one
    /// that stands for lines taken from the end of a file, and there is no row
    /// below it that a fold could hide.
    fn phantom_row_on_screen(&self, row: usize) -> bool {
        row >= self.display_map.buffer_line_count() || !self.display_map.is_buffer_line_hidden(row)
    }

    /// The display row a block above `row` is drawn at.
    fn phantom_display_row(&self, row: usize) -> usize {
        if row >= self.display_map.buffer_line_count() {
            self.display_map.display_row_count()
        } else {
            self.display_map.buffer_line_to_display_row(row)
        }
    }

    /// The rows of every block above buffer line `row` that does not sit on it.
    pub(super) fn phantom_rows_before(&self, row: usize) -> usize {
        rows_before(self.extras.phantom_lines(), row, |r| {
            self.phantom_row_on_screen(r)
        })
    }

    /// The rows of the block sitting directly above buffer line `row`.
    pub(super) fn phantom_rows_at(&self, row: usize) -> usize {
        rows_at(self.extras.phantom_lines(), row, |r| {
            self.phantom_row_on_screen(r)
        })
    }

    /// The block sitting directly above buffer line `row`, if it is drawn.
    pub(super) fn phantom_block_at(&self, row: usize) -> Option<&PhantomLines> {
        block_at(self.extras.phantom_lines(), row, |r| {
            self.phantom_row_on_screen(r)
        })
    }

    /// Every phantom row in the buffer.
    pub(super) fn phantom_rows_total(&self) -> usize {
        rows_total(self.extras.phantom_lines(), |r| {
            self.phantom_row_on_screen(r)
        })
    }

    /// The display row drawn at `visual_row`. See [`display_row_at_visual_row`].
    pub(super) fn display_row_at_visual_row(&self, visual_row: usize) -> usize {
        display_row_at_visual_row(
            self.extras.phantom_lines(),
            visual_row,
            |row| self.phantom_display_row(row),
            |row| self.phantom_row_on_screen(row),
            self.display_map.display_row_count(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(row: usize, rows: usize) -> PhantomLines {
        PhantomLines::new(row, vec!["gone".into(); rows], gpui::red())
    }

    /// A block sits above the row it names, so its rows count as "above" that
    /// row and as "before" every row under it.
    #[test]
    fn a_block_counts_above_its_own_row_and_before_the_rows_under_it() {
        let blocks = vec![block(3, 2), block(8, 1)];
        let shown = |_: usize| true;

        assert_eq!(rows_before(&blocks, 3, shown), 0);
        assert_eq!(rows_at(&blocks, 3, shown), 2);
        assert_eq!(rows_before(&blocks, 4, shown), 2);
        assert_eq!(rows_before(&blocks, 8, shown), 2);
        assert_eq!(rows_at(&blocks, 8, shown), 1);
        assert_eq!(rows_before(&blocks, 9, shown), 3);
        assert_eq!(rows_total(&blocks, shown), 3);
    }

    /// A block whose row a fold has hidden is hidden with it. Counting it would
    /// push every row below the fold down by a block nobody can see.
    #[test]
    fn a_block_on_a_folded_row_is_not_counted() {
        let blocks = vec![block(3, 2), block(8, 1)];
        let shown = |row: usize| row != 3;

        assert_eq!(rows_at(&blocks, 3, shown), 0);
        assert_eq!(rows_before(&blocks, 9, shown), 1);
        assert_eq!(rows_total(&blocks, shown), 1);
        assert!(block_at(&blocks, 3, shown).is_none());
        assert!(block_at(&blocks, 8, shown).is_some());
    }

    /// The reader's row and the buffer's row are the same until a block is
    /// passed, and a row inside a block belongs to the row the block sits above.
    #[test]
    fn a_visual_row_maps_back_to_the_display_row_drawn_there() {
        let blocks = vec![block(3, 2)];
        let same = |row: usize| row;
        let shown = |_: usize| true;

        // Rows above the block are untouched.
        assert_eq!(display_row_at_visual_row(&blocks, 0, same, shown, 10), 0);
        assert_eq!(display_row_at_visual_row(&blocks, 2, same, shown, 10), 2);
        // Rows 3 and 4 are the block itself, and belong to line 3.
        assert_eq!(display_row_at_visual_row(&blocks, 3, same, shown, 10), 3);
        assert_eq!(display_row_at_visual_row(&blocks, 4, same, shown, 10), 3);
        // Line 3's own text is at visual row 5, and everything below shifts.
        assert_eq!(display_row_at_visual_row(&blocks, 5, same, shown, 10), 3);
        assert_eq!(display_row_at_visual_row(&blocks, 6, same, shown, 10), 4);
        // Past the end it clamps to the last row rather than running off.
        assert_eq!(display_row_at_visual_row(&blocks, 99, same, shown, 10), 9);
    }

    /// Two blocks stack, and the second one's rows are found past the first.
    #[test]
    fn two_blocks_each_shift_what_is_under_them() {
        let blocks = vec![block(1, 1), block(4, 2)];
        let same = |row: usize| row;
        let shown = |_: usize| true;

        assert_eq!(display_row_at_visual_row(&blocks, 1, same, shown, 20), 1);
        assert_eq!(display_row_at_visual_row(&blocks, 2, same, shown, 20), 1);
        assert_eq!(display_row_at_visual_row(&blocks, 3, same, shown, 20), 2);
        // Line 4 sits at 4 + 1 phantom row = 5; its block covers 5 and 6.
        assert_eq!(display_row_at_visual_row(&blocks, 5, same, shown, 20), 4);
        assert_eq!(display_row_at_visual_row(&blocks, 6, same, shown, 20), 4);
        assert_eq!(display_row_at_visual_row(&blocks, 7, same, shown, 20), 4);
        assert_eq!(display_row_at_visual_row(&blocks, 8, same, shown, 20), 5);
    }

    /// An empty list is the common case, and must cost nothing and change
    /// nothing.
    #[test]
    fn no_blocks_leaves_every_row_where_it_was() {
        let none: Vec<PhantomLines> = Vec::new();
        let same = |row: usize| row;
        let shown = |_: usize| true;

        assert_eq!(rows_total(&none, shown), 0);
        assert_eq!(rows_before(&none, 5, shown), 0);
        assert_eq!(display_row_at_visual_row(&none, 7, same, shown, 10), 7);
    }
}
