use std::ops::Range;
use std::sync::{Arc, Mutex};

use gpui::{
    App, Bounds, HighlightStyle, Hsla, Pixels, Rems, SharedString, StyleRefinement, px, rems,
};

use crate::{ActiveTheme as _, highlighter::HighlightTheme};

/// What a find bar is looking for in a rendered document.
///
/// A rendered document has no lines and no offsets a caller can point at, so
/// the query itself is what is handed to the view: every occurrence of it in
/// the text the reader sees is painted in `background`, and the one the reader
/// is on — `current`, counting from the start of the document — in
/// `current_background`.
///
/// How many occurrences there are, and where the current one ended up on
/// screen, are known only once the document has been painted. They are put in
/// [`TextSearchResults`], which the caller keeps and reads on the frame after
/// it asked.
///
/// A paragraph that mixes an inline image with its text is laid out a line at
/// a time, so an occurrence there that straddles a line break is counted as
/// two. Every other paragraph holds its text in one piece and is exact.
#[derive(Clone)]
pub struct TextSearch {
    /// The text to look for. An empty query finds nothing.
    pub query: SharedString,
    /// Match regardless of the case of ASCII letters. True by default, which
    /// is what a find bar does.
    pub case_insensitive: bool,
    /// Which occurrence, counting from the start of the document, the reader is
    /// on. Out of range means no occurrence is the current one.
    pub current: usize,
    /// The colour every occurrence is painted in. Painted over the text, so it
    /// wants an alpha the way a selection does.
    pub background: Hsla,
    /// The colour the current occurrence is painted in instead.
    pub current_background: Hsla,
    /// Where the answers are put.
    pub results: TextSearchResults,
}

/// What a document held the last time it was painted for a [`TextSearch`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextSearchResult {
    /// How many occurrences of the query the document holds.
    pub count: usize,
    /// Where the current occurrence was painted, in window coordinates. `None`
    /// when there is no current occurrence — an empty query, or an index past
    /// the end.
    pub current: Option<Bounds<Pixels>>,
}

/// The handle a caller keeps to read what its [`TextSearch`] found.
///
/// Cheap to clone: every clone reads and writes the same answer.
#[derive(Clone, Default)]
pub struct TextSearchResults(Arc<Mutex<TextSearchResult>>);

impl std::fmt::Debug for TextSearchResults {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("TextSearchResults")
            .field(&self.get())
            .finish()
    }
}

impl TextSearchResults {
    /// What the last painted frame found.
    pub fn get(&self) -> TextSearchResult {
        self.0.lock().map(|found| *found).unwrap_or_default()
    }

    /// Start counting again, at the top of the document.
    pub(crate) fn begin(&self) {
        if let Ok(mut found) = self.0.lock() {
            *found = TextSearchResult::default();
        }
    }

    /// Claim `count` occurrences, and answer the ordinal of the first of them.
    pub(crate) fn claim(&self, count: usize) -> usize {
        let Ok(mut found) = self.0.lock() else {
            return usize::MAX;
        };
        let first = found.count;
        found.count += count;
        first
    }

    pub(crate) fn set_current(&self, bounds: Bounds<Pixels>) {
        if let Ok(mut found) = self.0.lock() {
            found.current = Some(bounds);
        }
    }
}

impl TextSearch {
    /// Look for `query`, painting every occurrence in `background`.
    pub fn new(query: impl Into<SharedString>, background: Hsla) -> Self {
        Self {
            query: query.into(),
            case_insensitive: true,
            current: 0,
            background,
            current_background: background,
            results: TextSearchResults::default(),
        }
    }

    /// Which occurrence the reader is on, counting from the start.
    pub fn current(mut self, current: usize) -> Self {
        self.current = current;
        self
    }

    /// The colour the current occurrence is painted in.
    pub fn current_background(mut self, background: Hsla) -> Self {
        self.current_background = background;
        self
    }

    /// Tell apart `Query` from `query`.
    pub fn case_sensitive(mut self) -> Self {
        self.case_insensitive = false;
        self
    }

    /// Read the answers through this handle rather than a fresh one.
    pub fn results(mut self, results: TextSearchResults) -> Self {
        self.results = results;
        self
    }

    /// Where `text` holds the query.
    pub(crate) fn matches_in(&self, text: &str) -> Vec<Range<usize>> {
        matches_in(text, &self.query, self.case_insensitive)
    }
}

impl PartialEq for TextSearch {
    fn eq(&self, other: &Self) -> bool {
        self.query == other.query
            && self.case_insensitive == other.case_insensitive
            && self.current == other.current
            && self.background == other.background
            && self.current_background == other.current_background
    }
}

/// Every place `query` occurs in `text`, left to right and never overlapping.
///
/// Byte ranges of `text`, so the caller can slice with them. The case-blind
/// comparison covers ASCII letters only — the same trade the editor's find bar
/// makes, and the reason the answers stay byte-exact: folding the case of the
/// whole text first would move every offset after a character whose lower case
/// is a different length.
///
/// A window that starts in the middle of a character can never match: its first
/// byte is a continuation byte, and the first byte of the query is either ASCII
/// or the start of a character.
pub(crate) fn matches_in(text: &str, query: &str, case_insensitive: bool) -> Vec<Range<usize>> {
    if query.is_empty() || query.len() > text.len() {
        return Vec::new();
    }

    let (haystack, needle) = (text.as_bytes(), query.as_bytes());
    let mut found = Vec::new();
    let mut ix = 0;
    while ix + needle.len() <= haystack.len() {
        let window = &haystack[ix..ix + needle.len()];
        let hit = if case_insensitive {
            window.eq_ignore_ascii_case(needle)
        } else {
            window == needle
        };
        if hit {
            found.push(ix..ix + needle.len());
            ix += needle.len();
        } else {
            ix += 1;
        }
    }
    found
}

/// TextViewStyle used to customize the style for [`TextView`].
#[derive(Clone)]
pub struct TextViewStyle {
    /// Where a document's own image paths are read from, if anywhere.
    ///
    /// `None`, the default, is what keeps a document from reaching the file
    /// system: every image URL stays URI-backed, so a `file://` or a bare path
    /// in untrusted Markdown loads nothing.
    ///
    /// Set it to a directory to say "this document is a file on disk, and I
    /// trust it": an image path is then read from disk, and a relative one is
    /// resolved against this directory. It is what an editor or a viewer
    /// showing a local Markdown file wants, and what a chat window rendering
    /// someone else's Markdown must not set.
    pub image_base: Option<Arc<std::path::Path>>,
    /// Ranges of the source this document was parsed from that a caller wants
    /// marked, each with the colour to mark it in.
    ///
    /// Empty by default, which draws nothing. A top-level block whose span
    /// meets one of these ranges gets a bar in the margin beside it, the way a
    /// code editor marks a changed line in its gutter — except that prose has
    /// no lines to mark, so the block is the smallest thing there is.
    ///
    /// The bar is drawn outside the block's own box, so a document with marks
    /// lays out exactly like one without.
    pub marked_ranges: Arc<Vec<(std::ops::Range<usize>, Hsla)>>,
    /// What a find bar is looking for in this document, if anything.
    ///
    /// `None`, the default, paints nothing and costs nothing. See
    /// [`TextSearch`].
    pub search: Option<TextSearch>,
    /// Gap of each paragraphs, default is 1 rem.
    pub paragraph_gap: Rems,
    /// Base font size for headings, default is 14px.
    pub heading_base_font_size: Pixels,
    /// Function to calculate heading font size based on heading level (1-6).
    ///
    /// The first parameter is the heading level (1-6), the second parameter is the base font size.
    /// The second parameter is the base font size.
    pub heading_font_size: Option<Arc<dyn Fn(u8, Pixels) -> Pixels + Send + Sync + 'static>>,
    /// Highlight theme for code blocks. Default: [`HighlightTheme::default_light()`]
    pub highlight_theme: Arc<HighlightTheme>,
    /// The style refinement for code blocks.
    pub code_block: StyleRefinement,
    /// Style refinement applied to the table container (the bordered wrapper
    /// in wrap mode, the scroll viewport in horizontal-scroll mode).
    ///
    /// Set `overflow_x: scroll` here for adaptive table layout: columns fit
    /// their content when space allows, shrink (wrapping cell text) down to a
    /// per-column floor when the frame is narrower, and below that the table
    /// scrolls horizontally instead of squeezing further, e.g.
    /// `TextViewStyle::default().table({ let mut s = StyleRefinement::default(); s.overflow.x = Some(Overflow::Scroll); s })`.
    pub table: StyleRefinement,
    /// Style refinement applied to each table cell.
    ///
    /// With the scroll layout, set `white_space: nowrap` here to keep cells
    /// on a single line — columns then never shrink and the table scrolls as
    /// soon as the content is wider than the frame.
    pub table_cell: StyleRefinement,
    /// The highlight style for inline code.
    ///
    /// Default is [`HighlightStyle::default()`], the `background_color` will
    /// fallback to `cx.theme().accent`, if it is `None`.
    pub inline_code: HighlightStyle,
    pub is_dark: bool,
}

impl PartialEq for TextViewStyle {
    fn eq(&self, other: &Self) -> bool {
        // `marked_ranges` and `search` are deliberately left out: this
        // comparison is what bumps the selection revision, and neither of them
        // changes a single character of the text a selection is taken from.
        // The find bar changes its query on every keystroke, and a reader's
        // selection must survive that.
        self.image_base == other.image_base
            && self.paragraph_gap == other.paragraph_gap
            && self.heading_base_font_size == other.heading_base_font_size
            && match (&self.heading_font_size, &other.heading_font_size) {
                (Some(left), Some(right)) => (1..=6).all(|level| {
                    left(level, self.heading_base_font_size)
                        == right(level, other.heading_base_font_size)
                }),
                (None, None) => true,
                _ => false,
            }
            && self.highlight_theme == other.highlight_theme
            && self.code_block == other.code_block
            && self.table == other.table
            && self.table_cell == other.table_cell
            && self.inline_code == other.inline_code
            && self.is_dark == other.is_dark
    }
}

impl Default for TextViewStyle {
    fn default() -> Self {
        Self {
            image_base: None,
            marked_ranges: Arc::new(Vec::new()),
            search: None,
            paragraph_gap: rems(1.),
            heading_base_font_size: px(14.),
            heading_font_size: None,
            highlight_theme: HighlightTheme::default_light().clone(),
            code_block: StyleRefinement::default(),
            table: StyleRefinement::default(),
            table_cell: StyleRefinement::default(),
            inline_code: HighlightStyle::default(),
            is_dark: false,
        }
    }
}

impl TextViewStyle {
    /// Set paragraph gap, default is 1 rem.
    pub fn paragraph_gap(mut self, gap: Rems) -> Self {
        self.paragraph_gap = gap;
        self
    }

    pub fn heading_font_size<F>(mut self, f: F) -> Self
    where
        F: Fn(u8, Pixels) -> Pixels + Send + Sync + 'static,
    {
        self.heading_font_size = Some(Arc::new(f));
        self
    }

    /// Set style for code blocks.
    pub fn code_block(mut self, style: StyleRefinement) -> Self {
        self.code_block = style;
        self
    }

    /// Set style for inline code spans.
    pub fn inline_code(mut self, style: HighlightStyle) -> Self {
        self.inline_code = style;
        self
    }

    /// Set extra style for the table container.
    ///
    /// Set `overflow_x: scroll` on the refinement for adaptive layout: cells
    /// wrap as the frame narrows, and once columns reach their minimum width
    /// the table scrolls horizontally instead of shrinking further.
    pub fn table(mut self, style: StyleRefinement) -> Self {
        self.table = style;
        self
    }

    /// Set extra style for each table cell.
    ///
    /// With the scroll table layout, `white_space: nowrap` here keeps cells
    /// on a single line and the table scrolls whenever the content is wider
    /// than the frame.
    pub fn table_cell(mut self, style: StyleRefinement) -> Self {
        self.table_cell = style;
        self
    }

    /// Returns the [`HighlightStyle`] to use for inline code,
    /// fallback `background_color` to `cx.theme().accent`, if it is `None`.
    pub(crate) fn inline_code_highlight(&self, cx: &App) -> HighlightStyle {
        let mut style = self.inline_code;
        if style.background_color.is_none() {
            style.background_color = Some(cx.theme().accent);
        }
        style
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_layout_fingerprint_covers_callback_table_and_theme_fields() {
        let base = TextViewStyle::default();
        let heading = base.clone().heading_font_size(|_, size| size);
        assert!(heading == base.clone().heading_font_size(|_, size| size));
        assert!(heading != base.clone().heading_font_size(|_, size| size * 2.));

        let mut table = StyleRefinement::default();
        table.text.white_space = Some(gpui::WhiteSpace::Nowrap);
        assert!(base != base.clone().table_cell(table));

        let mut dark = base.clone();
        dark.is_dark = true;
        assert!(base != dark);
    }

    #[test]
    fn cloning_preserves_the_same_heading_callback_fingerprint() {
        let style = TextViewStyle::default().heading_font_size(|_, size| size);
        assert!(style == style.clone());
    }

    #[test]
    fn occurrences_are_found_left_to_right_and_never_overlap() {
        assert_eq!(
            matches_in("a cat and a cat", "cat", true),
            vec![2..5, 12..15]
        );
        assert_eq!(matches_in("aaaa", "aa", true), vec![0..2, 2..4]);
    }

    #[test]
    fn the_case_of_a_letter_does_not_hide_an_occurrence() {
        assert_eq!(
            matches_in("Cat CAT cat", "cat", true),
            vec![0..3, 4..7, 8..11]
        );
        assert_eq!(matches_in("Cat CAT cat", "cat", false), vec![8..11]);
    }

    /// The ranges are byte ranges of the text as it is, so they can slice it.
    #[test]
    fn an_occurrence_after_a_wide_character_keeps_its_byte_offsets() {
        let text = "héllo cat";
        let found = matches_in(text, "cat", true);
        assert_eq!(found, vec![7..10]);
        assert_eq!(&text[found[0].clone()], "cat");
    }

    /// A window that starts inside a character cannot match: it begins with a
    /// continuation byte, and no query does.
    #[test]
    fn a_query_never_matches_the_middle_of_a_character() {
        assert!(matches_in("日本語", "本", true).len() == 1);
        assert!(matches_in("日本語", "\u{fffd}", true).is_empty());
    }

    #[test]
    fn nothing_is_found_for_an_empty_query_or_a_query_longer_than_the_text() {
        assert!(matches_in("cat", "", true).is_empty());
        assert!(matches_in("cat", "cats", true).is_empty());
    }
}
