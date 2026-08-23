use gpui::{
    App, Div, Hsla, InteractiveElement as _, IntoElement, ListState, ParentElement as _,
    SharedString, Styled as _, Window, canvas, div, px,
};

use std::ops::RangeInclusive;

use crate::text::{
    SelectionFormat,
    node::{BlockNode, NodeContext},
};

/// The parsed document AST.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct ParsedDocument {
    pub(crate) source: SharedString,
    pub(crate) blocks: Vec<BlockNode>,
}

#[derive(Default, Clone, Copy)]
pub(crate) struct NodeRenderOptions {
    pub(crate) ix: usize,
    pub(crate) in_list: bool,
    pub(crate) todo: bool,
    pub(crate) ordered: bool,
    pub(crate) depth: usize,
    pub(crate) is_last: bool,
}

impl NodeRenderOptions {
    pub(crate) fn is_last(mut self, is_last: bool) -> Self {
        self.is_last = is_last;
        self
    }
}

impl ParsedDocument {
    pub(super) fn text(&self) -> String {
        let mut text = String::new();
        for block in self.blocks.iter() {
            text.push_str(&block.text());
        }
        text
    }

    /// The selected text across all blocks, in `format`.
    ///
    /// In [`SelectionFormat::Source`] each block reconstructs its own Markdown
    /// source (inline markup, and block prefixes for headings and lists), and
    /// top-level blocks are joined with a blank line so the result re-renders
    /// with the same block structure.
    ///
    /// A block only learns its selection when it is painted, so in a scrollable
    /// (virtualized) view every block the user scrolled past reports nothing.
    /// The selection is one continuous range, so blocks it spans that came up
    /// empty are inside it and are emitted whole rather than dropped. `blocks`
    /// bounds that span; it comes from the selection endpoints, which hold on to
    /// their block index even after it scrolls out of view (the painted blocks
    /// alone cannot bound the span, because the press that starts a drag leaves
    /// an empty selection that never reaches paint). Without it, fall back to
    /// the painted blocks. (Source mode is only used by non-scrollable views,
    /// where `blocks` is `None` and every block paints.)
    ///
    /// A standalone image (a paragraph that is only an image) has no selectable
    /// text run, so it never carries a selection of its own. It is therefore
    /// included when it is *enclosed* by the selection — some block before and
    /// some block after it are selected — mirroring how an inline image is
    /// emitted when the selection runs into it. (Select-all returns the source
    /// verbatim, so a leading or trailing image is still copied there.)
    pub(super) fn selected_text(
        &self,
        format: SelectionFormat,
        blocks: Option<RangeInclusive<usize>>,
    ) -> String {
        let requested_blocks = blocks.clone();
        let painted = self
            .blocks
            .iter()
            .map(|block| block.has_selection())
            .collect::<Vec<_>>();
        let (Some(painted_first), Some(painted_last)) = (
            painted.iter().position(|painted| *painted),
            painted.iter().rposition(|painted| *painted),
        ) else {
            return String::new();
        };

        let last_ix = self.blocks.len().saturating_sub(1);
        let (first, last) = match blocks {
            Some(blocks) => (*blocks.start().min(&last_ix), *blocks.end().min(&last_ix)),
            None => (painted_first, painted_last),
        };

        if format == SelectionFormat::Plain {
            let mut text = String::new();
            for (ix, block) in self.blocks.iter().enumerate().take(last + 1).skip(first) {
                let selected = block.selected_text(format);
                let is_virtual_endpoint = requested_blocks
                    .as_ref()
                    .is_some_and(|blocks| ix == *blocks.start() || ix == *blocks.end());
                if requested_blocks.is_some() && !is_virtual_endpoint {
                    text.push_str(&block.text());
                } else if !selected.is_empty() {
                    text.push_str(&selected);
                } else if !painted[ix] {
                    // Never painted, so it cannot report a selection of its own
                    // even though the span covers it. A painted block that came
                    // up empty really has nothing selected, and stays empty.
                    text.push_str(&block.text());
                }
            }
            return text;
        }

        let mut out: Vec<String> = Vec::new();
        for (ix, block) in self.blocks.iter().enumerate().take(last + 1).skip(first) {
            // The selection is one continuous range, so only the block it
            // starts in and the block it ends in can be partly selected.
            // Everything between them is covered whole, and so is any block
            // that reports nothing — it either scrolled past without painting,
            // or renders no selectable text run at all (a rule, a break, a
            // custom node, a standalone image).
            let source = if (ix == first || ix == last) && painted[ix] {
                block.selected_text(format)
            } else {
                self.whole_source(block)
            };

            let trimmed = source.trim_end_matches('\n');
            if !trimmed.is_empty() {
                out.push(trimmed.to_string());
            }
        }
        out.join("\n\n")
    }

    /// The whole source of a block the selection covers.
    ///
    /// Copied straight out of the original text, which the Markdown parser
    /// locates per block. That keeps whatever the author wrote — `_italic_`
    /// stays `_italic_`, a reference link keeps its `[ref]` form, a table keeps
    /// its column padding — and it needs no rule of its own per block type.
    /// Blocks the parser could not locate fall back to reconstruction.
    fn whole_source(&self, block: &BlockNode) -> String {
        if let Some(span) = block.span()
            && let Some(source) = self.source.get(span.start..span.end)
        {
            return source.to_string();
        }

        block.selected_text(SelectionFormat::Source)
    }

    /// Synchronously clear the selection stored in every inline state.
    ///
    /// This mirrors the [`selected_text`](Self::selected_text) traversal so the
    /// stored selection can be cleared without relying on a repaint. Offscreen
    /// (virtualized) views do not repaint, so their `InlineState.selection`
    /// would otherwise retain stale values from the last painted frame.
    pub(super) fn clear_selection(&self) {
        for block in self.blocks.iter() {
            block.clear_selection();
        }
    }

    /// Converts the node to markdown format.
    ///
    /// This is used to generate markdown for test.
    #[allow(dead_code)]
    pub(crate) fn to_markdown(&self) -> String {
        self.blocks
            .iter()
            .map(|child| child.to_markdown())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub(super) fn render_root(
        &self,
        list_state: Option<ListState>,
        node_cx: &NodeContext,
        window: &mut Window,
        cx: &mut App,
    ) -> impl IntoElement {
        let Some(list_state) = list_state else {
            let blocks_len = self.blocks.len();
            return div()
                .id("document")
                .children(self.blocks.iter().enumerate().map(move |(ix, node)| {
                    let is_last = ix + 1 == blocks_len;
                    let mark = mark_for(node, node_cx);
                    anchored(
                        marked(
                            node.render_block(
                                NodeRenderOptions {
                                    ix,
                                    is_last,
                                    ..Default::default()
                                },
                                node_cx,
                                window,
                                cx,
                            ),
                            mark,
                        ),
                        node,
                        node_cx,
                    )
                }));
        };

        let options = NodeRenderOptions {
            is_last: true,
            ..Default::default()
        };

        let blocks = &self.blocks;

        if list_state.item_count() != blocks.len() {
            list_state.reset(blocks.len());
        }

        div().id("document").size_full().child(
            gpui::list(list_state, {
                let node_cx = node_cx.clone();
                let blocks = blocks.clone();
                move |ix, window, cx| {
                    let is_last = ix + 1 == blocks.len();
                    let mark = mark_for(&blocks[ix], &node_cx);
                    anchored(
                        marked(
                            blocks[ix].render_block(
                                NodeRenderOptions {
                                    ix,
                                    is_last,
                                    ..options
                                },
                                &node_cx,
                                window,
                                cx,
                            ),
                            mark,
                        ),
                        &blocks[ix],
                        &node_cx,
                    )
                    .into_any_element()
                }
            })
            .size_full(),
        )
    }
}

/// The colour a top-level block is marked in, if any.
///
/// A block is marked when its span meets one of `TextViewStyle::marked_ranges`.
/// "Meets" rather than "is inside": a change of one word marks the paragraph
/// holding it, which is the smallest thing prose has to point at.
///
/// The first range that meets it wins. Ranges are given by the caller in the
/// order it wants them tried, and a block that is both changed and new is one
/// the caller has already decided about.
fn mark_for(node: &BlockNode, node_cx: &NodeContext) -> Option<Hsla> {
    if node_cx.style.marked_ranges.is_empty() {
        return None;
    }
    let span = node.span()?;
    node_cx
        .style
        .marked_ranges
        .iter()
        .find(|(range, _)| span.start < range.end && range.start < span.end)
        .map(|(_, color)| *color)
}

/// Note where this block was drawn, for a caller that asked for anchors.
///
/// The canvas draws nothing and is taken out of the flow, so an anchored
/// document lays out exactly like one without anchors — the same arrangement
/// the margin bar in [`marked`] uses. It is given an inset as well as a size:
/// an absolutely positioned element with no inset is laid out where it would
/// have sat in flow, which here is one whole block below the block it is
/// measuring.
///
/// A block with no span is skipped rather than recorded at offset zero, which
/// would put a second answer on the top of the document.
fn anchored(block: Div, node: &BlockNode, node_cx: &NodeContext) -> Div {
    let (Some(anchors), Some(span)) = (node_cx.style.anchors.clone(), node.span()) else {
        return block;
    };
    let source = span.start;
    block.relative().child(
        canvas(
            move |bounds, _, _| anchors.record(source, bounds),
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full(),
    )
}

/// Put `block` in a box that carries a bar in the margin beside it.
///
/// The bar is absolutely positioned outside the box, so an unmarked document
/// and a marked one lay out identically — nothing shifts when the marks arrive
/// or go away.
fn marked(block: impl IntoElement, mark: Option<Hsla>) -> Div {
    let block = div().child(block);
    let Some(color) = mark else {
        return block;
    };
    block.relative().child(
        div()
            .absolute()
            .left(px(-14.))
            .top_0()
            .bottom_0()
            .w(px(2.))
            .rounded_full()
            .bg(color),
    )
}
