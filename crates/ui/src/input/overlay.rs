use std::collections::HashMap;

use gpui::{AnyElement, App, Entity, EntityId, Global, IntoElement, WeakEntity, Window};
use ropey::Rope;

use super::{
    InputBaseState, InputModeKind,
    popovers::{CodeActionMenu, CompletionMenu, DiagnosticPopover, HoverPopover},
    search::SearchPanel,
};

struct InputOverlayRegistry<M: OverlayMode> {
    hosts: HashMap<EntityId, (WeakEntity<InputBaseState<M>>, InputOverlayHost<M>)>,
}

impl<M: OverlayMode> Default for InputOverlayRegistry<M> {
    fn default() -> Self {
        Self {
            hosts: HashMap::default(),
        }
    }
}

impl<M: OverlayMode> Global for InputOverlayRegistry<M> {}

struct InputOverlayHost<M: OverlayMode> {
    search: Entity<SearchPanel<M>>,
    /// What makes the bar appear, or appear again: whether it is open, which
    /// half it opens on, where it was anchored, and how many times it has been
    /// opened. The query is deliberately not here — see `search_query`.
    search_signature: (bool, bool, Option<usize>, u64),
    /// The query the panel was last told about.
    ///
    /// A query typed into the field arrives back here on the next frame, and
    /// the field already holds it, so nothing is written back. A query set from
    /// outside the field does not, and is carried into it.
    search_query: String,
    /// The language-feature popovers. Only a code editor has them.
    lsp: Option<LspOverlays>,
}

/// Identifies one version of an overlay's content.
///
/// Sync runs on every frame, so the check for "did this change" must not touch
/// the content itself: the completion list can hold hundreds of items, each
/// with its own documentation. The engine bumps a revision when it swaps the
/// content, and the popovers are keyed by cheap identity instead — a revision,
/// an `Rc` pointer, or a range.
#[derive(PartialEq, Eq, Default)]
struct OverlaySignature {
    open: bool,
    revision: u64,
}

/// The popovers driven by language features: completion, code actions, hover
/// and diagnostics. They belong to a code editor, so they are built and synced
/// by [`OverlayMode`], where the state's kind is concrete.
pub(crate) struct LspOverlays {
    completion: Entity<CompletionMenu>,
    code_actions: Entity<CodeActionMenu>,
    hover: Option<Entity<HoverPopover>>,
    diagnostic: Option<Entity<DiagnosticPopover>>,
    completion_signature: OverlaySignature,
    code_action_signature: OverlaySignature,
    hover_signature: Option<std::ops::Range<usize>>,
    diagnostic_signature: Option<std::rc::Rc<gpui_base::input::DiagnosticEntry>>,
}

/// What the state read out of the engine for one sync pass.
///
/// Deliberately free of content: only what is needed to decide whether a
/// popover is shown and whether it changed. The content is read from the state
/// again, on the frames where it actually did.
pub(crate) struct LspSnapshot {
    completion: OverlaySignature,
    completion_start: Option<usize>,
    code_action: OverlaySignature,
    hover: Option<std::ops::Range<usize>>,
    diagnostic: Option<std::rc::Rc<gpui_base::input::DiagnosticEntry>>,
    cursor: usize,
}

impl LspSnapshot {
    fn has_overlay(&self) -> bool {
        self.completion.open
            || self.code_action.open
            || self.hover.is_some()
            || self.diagnostic.is_some()
    }
}

/// What the UI layer needs from each kind of input.
///
/// The UI layer is generic over the mode, so it cannot name the editor's state
/// type. This trait is the seam: the code-editor implementation is the only one
/// that builds language-feature popovers, and inside it the state is an
/// `Entity<EditorState>`, which those popovers take.
///
/// Entirely internal: nothing public is generic over the mode any more.
pub(crate) trait OverlayMode: InputModeKind + Sized {
    /// Reads the language-feature state this mode shows, if any.
    fn lsp_snapshot(_state: &InputBaseState<Self>, _cx: &App) -> Option<LspSnapshot> {
        None
    }

    /// Installs the routing that lets an open menu consume actions first.
    fn install_action_handler(_state: &Entity<InputBaseState<Self>>, _cx: &mut App) {}

    fn build_lsp(
        _state: &Entity<InputBaseState<Self>>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<LspOverlays> {
        None
    }

    fn sync_lsp(
        _lsp: &mut LspOverlays,
        _state: &Entity<InputBaseState<Self>>,
        _snapshot: &LspSnapshot,
        _window: &mut Window,
        _cx: &mut App,
    ) {
    }
}

impl OverlayMode for crate::input::InputMode {}
impl OverlayMode for crate::input::TextareaMode {}

impl OverlayMode for crate::input::EditorMode {
    fn install_action_handler(state: &Entity<InputBaseState<Self>>, cx: &mut App) {
        let id = state.entity_id();
        state.update(cx, move |state, _| {
            state.set_overlay_action_handler(move |kind, action, window, cx| {
                let menus = cx
                    .try_global::<InputOverlayRegistry<Self>>()
                    .and_then(|registry| registry.hosts.get(&id))
                    .and_then(|(_, host)| host.lsp.as_ref())
                    .map(|lsp| (lsp.completion.clone(), lsp.code_actions.clone()));
                let Some((completion, code_actions)) = menus else {
                    return false;
                };
                match kind {
                    gpui_base::input::InputOverlayKind::Completion => {
                        completion.update(cx, |menu, cx| menu.handle_action(action, window, cx))
                    }
                    gpui_base::input::InputOverlayKind::CodeAction => {
                        code_actions.update(cx, |menu, cx| menu.handle_action(action, window, cx))
                    }
                }
            });
        });
    }

    fn lsp_snapshot(state: &InputBaseState<Self>, _cx: &App) -> Option<LspSnapshot> {
        let completion = state.completion_menu_state();
        let code_actions = state.code_action_menu_state();
        Some(LspSnapshot {
            completion: OverlaySignature {
                open: completion.open,
                revision: completion.revision(),
            },
            completion_start: completion.trigger_start_offset,
            code_action: OverlaySignature {
                open: code_actions.open,
                revision: code_actions.revision(),
            },
            hover: state
                .hover_popover()
                .map(|popover| popover.symbol_range.clone()),
            diagnostic: state.diagnostic_popover(),
            cursor: state.cursor(),
        })
    }

    fn build_lsp(
        state: &Entity<InputBaseState<Self>>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<LspOverlays> {
        Some(LspOverlays {
            completion: CompletionMenu::new(state.clone(), window, cx),
            code_actions: CodeActionMenu::new(state.clone(), window, cx),
            hover: None,
            diagnostic: None,
            completion_signature: OverlaySignature::default(),
            code_action_signature: OverlaySignature::default(),
            hover_signature: None,
            diagnostic_signature: None,
        })
    }

    fn sync_lsp(
        lsp: &mut LspOverlays,
        state: &Entity<InputBaseState<Self>>,
        snapshot: &LspSnapshot,
        window: &mut Window,
        cx: &mut App,
    ) {
        let cursor = snapshot.cursor;

        if snapshot.completion != lsp.completion_signature {
            lsp.completion_signature = OverlaySignature {
                open: snapshot.completion.open,
                revision: snapshot.completion.revision,
            };
            let open = snapshot.completion.open;
            let start = snapshot.completion_start;
            // Read the items only now, on a frame where they changed.
            let (query, items) = {
                let menu = state.read(cx).completion_menu_state();
                (menu.query.clone(), menu.items.clone())
            };
            lsp.completion.update(cx, |menu, cx| {
                if open {
                    menu.update_query(start.unwrap_or(cursor), query);
                    menu.show(cursor, items, window, cx);
                } else {
                    menu.hide(cx);
                }
            });
        }

        if snapshot.code_action != lsp.code_action_signature {
            lsp.code_action_signature = OverlaySignature {
                open: snapshot.code_action.open,
                revision: snapshot.code_action.revision,
            };
            let open = snapshot.code_action.open;
            let items = state.read(cx).code_action_menu_state().items.clone();
            lsp.code_actions.update(cx, |menu, cx| {
                if open {
                    menu.show(cursor, items, window, cx);
                } else {
                    menu.hide(cx);
                }
            });
        }

        // A hover popover is anchored to one symbol, so a new range means new
        // content and the same range means the same content.
        if snapshot.hover != lsp.hover_signature {
            lsp.hover_signature = snapshot.hover.clone();
            let popover = state
                .read(cx)
                .hover_popover()
                .map(|popover| (popover.symbol_range.clone(), popover.hover.clone()));
            lsp.hover = popover.map(|(symbol_range, hover)| {
                HoverPopover::new(state.clone(), symbol_range, &hover, cx)
            });
        }

        // The engine hands out the diagnostic behind an `Rc`, so pointer
        // identity already answers "is this the same entry".
        let diagnostic_changed = match (&snapshot.diagnostic, &lsp.diagnostic_signature) {
            (Some(new), Some(old)) => !std::rc::Rc::ptr_eq(new, old),
            (None, None) => false,
            _ => true,
        };
        if diagnostic_changed {
            lsp.diagnostic_signature = snapshot.diagnostic.clone();
            lsp.diagnostic = snapshot
                .diagnostic
                .as_deref()
                .map(|entry| DiagnosticPopover::new(entry, state.clone(), cx));
        }
    }
}

#[derive(Default)]
pub(super) struct InputOverlays {
    pub search: Option<AnyElement>,
    pub floating: Vec<AnyElement>,
}

impl InputOverlays {
    #[cfg(test)]
    fn len(&self) -> usize {
        usize::from(self.search.is_some()) + self.floating.len()
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.search.is_none() && self.floating.is_empty()
    }
}

impl<M: OverlayMode> InputOverlayHost<M> {
    fn new(state: Entity<InputBaseState<M>>, window: &mut Window, cx: &mut App) -> Self {
        Self {
            search: SearchPanel::new(state.clone(), window, cx),
            search_signature: (false, false, None, 0),
            search_query: String::new(),
            lsp: M::build_lsp(&state, window, cx),
        }
    }

    fn sync(
        &mut self,
        state: &Entity<InputBaseState<M>>,
        window: &mut Window,
        cx: &mut App,
    ) -> InputOverlays {
        let snapshot = M::lsp_snapshot(state.read(cx), cx);
        let (search_open, replace_mode, search_session) = {
            let state = state.read(cx);
            let search = state.search_session();
            (search.open, search.replace_mode, search.clone())
        };

        self.search
            .update(cx, |panel, _| panel.sync_session(&search_session));

        let search_signature = (
            search_open,
            replace_mode,
            search_session.anchor_offset,
            search_session.open_revision,
        );
        if search_signature != self.search_signature {
            self.search_signature = search_signature;
            self.search_query = search_session.query.clone();
            self.search.update(cx, |panel, cx| {
                if search_open {
                    let selected = Rope::from(search_session.query.clone());
                    let visible = search_session.anchor_offset.map(|offset| offset..offset);
                    panel.show_with_focus(
                        &selected,
                        replace_mode,
                        visible,
                        !cfg!(test),
                        window,
                        cx,
                    );
                } else {
                    panel.hide_with_focus(!cfg!(test), window, cx);
                }
            });
        } else if search_open && search_session.query != self.search_query {
            // The query changed with the bar left as it was. That is either the
            // reader typing — the field already holds it, and writing it back
            // would move the caret — or a caller setting it from outside, which
            // the field has to be told about.
            self.search_query = search_session.query.clone();
            let query = search_session.query.clone();
            self.search
                .update(cx, |panel, cx| panel.sync_query(&query, window, cx));
        }

        if let (Some(lsp), Some(snapshot)) = (self.lsp.as_mut(), snapshot.as_ref()) {
            M::sync_lsp(lsp, state, snapshot, window, cx);
        }

        let search = search_open.then(|| self.search.clone().into_any_element());
        let mut floating = Vec::with_capacity(4);
        if let (Some(lsp), Some(snapshot)) = (self.lsp.as_ref(), snapshot.as_ref()) {
            if snapshot.completion.open {
                floating.push(lsp.completion.clone().into_any_element());
            }
            if snapshot.code_action.open {
                floating.push(lsp.code_actions.clone().into_any_element());
            }
            if let Some(hover) = lsp.hover.as_ref() {
                floating.push(hover.clone().into_any_element());
            }
            if let Some(diagnostic) = lsp.diagnostic.as_ref() {
                floating.push(diagnostic.clone().into_any_element());
            }
        }
        InputOverlays { search, floating }
    }
}

pub(super) fn render_overlays<M: OverlayMode>(
    state: &Entity<InputBaseState<M>>,
    window: &mut Window,
    cx: &mut App,
) -> InputOverlays {
    M::install_action_handler(state, cx);
    let has_overlay = {
        let state = state.read(cx);
        state.search_session().open
            || M::lsp_snapshot(state, cx).is_some_and(|lsp| lsp.has_overlay())
    };
    if !has_overlay {
        if cx.has_global::<InputOverlayRegistry<M>>() {
            let registry = cx.global_mut::<InputOverlayRegistry<M>>();
            registry.hosts.remove(&state.entity_id());
            registry
                .hosts
                .retain(|_, (owner, _)| owner.upgrade().is_some());
        }
        return InputOverlays::default();
    }

    if !cx.has_global::<InputOverlayRegistry<M>>() {
        cx.set_global(InputOverlayRegistry::<M>::default());
    }

    let id = state.entity_id();
    let mut host = cx
        .global_mut::<InputOverlayRegistry<M>>()
        .hosts
        .remove(&id)
        .map(|(_, host)| host)
        .unwrap_or_else(|| InputOverlayHost::new(state.clone(), window, cx));
    let overlays = host.sync(state, window, cx);
    cx.global_mut::<InputOverlayRegistry<M>>()
        .hosts
        .insert(id, (state.downgrade(), host));
    overlays
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext as _, Render, SharedString, div};
    use gpui_base::input::CodeActionItem;
    use gpui_base::input::DiagnosticEntry;
    use lsp_types::{CodeAction, CompletionItem, Hover, HoverContents, MarkedString};

    struct OverlayProbe {
        state: Entity<crate::input::EditorState>,
    }

    impl Render for OverlayProbe {
        fn render(
            &mut self,
            _window: &mut Window,
            _cx: &mut gpui::Context<Self>,
        ) -> impl IntoElement {
            div()
        }
    }

    /// Typing a word into the find bar must leave that word in the field.
    ///
    /// The query is part of the signature that decides whether the panel is
    /// shown again, and showing it selects everything in the field — so every
    /// keystroke used to select what was typed so far, and the next character
    /// replaced it. Typing "zebra" left "a".
    #[gpui::test]
    fn typing_in_the_find_bar_builds_up_the_word(cx: &mut gpui::TestAppContext) {
        cx.update(crate::init);
        let (probe, cx) = cx.add_window_view(|window, cx| OverlayProbe {
            state: cx.new(|cx| crate::input::EditorState::new(window, cx).searchable(true)),
        });
        let state = probe.read_with(cx, |probe, _| probe.state.clone());

        cx.update(|window, cx| {
            state.update(cx, |state, cx| {
                state.set_value("hello world", window, cx);
                state.open_search(false, cx);
            });

            let mut host = InputOverlayHost::new(state.clone(), window, cx);
            host.sync(&state, window, cx);
            let input = host.search.read(cx).search_input().clone();

            // One character at a time, the way the field is actually filled:
            // the panel's own subscription hands each new value to the engine,
            // and the frame after it syncs.
            for ch in "zebra".chars() {
                input.update(cx, |input, cx| input.replace(ch.to_string(), window, cx));
                let query = input.read(cx).value().to_string();
                state.update(cx, |state, cx| state.set_search_query(query, true, cx));
                host.sync(&state, window, cx);
                assert!(
                    input.read(cx).selected_range().is_empty(),
                    "the field must not select itself while it is being typed in"
                );
            }

            assert_eq!(input.read(cx).value(), "zebra");
            assert_eq!(state.read(cx).search_session().query, "zebra");
            assert!(
                state.read(cx).search_session().matcher.is_empty(),
                "a word the document does not hold is still a word the reader may type"
            );
        });
    }

    /// Opening the bar over an already-open bar still offers the old query for
    /// replacing, and a query set from outside the field still reaches it.
    ///
    /// Both used to come free of the query being part of the signature. They
    /// are what the open count and `sync_query` are for now.
    #[gpui::test]
    fn the_field_is_re_selected_on_a_second_open_and_follows_the_engine(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(crate::init);
        let (probe, cx) = cx.add_window_view(|window, cx| OverlayProbe {
            state: cx.new(|cx| crate::input::EditorState::new(window, cx).searchable(true)),
        });
        let state = probe.read_with(cx, |probe, _| probe.state.clone());

        cx.update(|window, cx| {
            state.update(cx, |state, cx| {
                state.set_value("hello world", window, cx);
                state.open_search(false, cx);
            });
            let mut host = InputOverlayHost::new(state.clone(), window, cx);
            host.sync(&state, window, cx);
            let input = host.search.read(cx).search_input().clone();

            input.update(cx, |input, cx| input.replace("world", window, cx));
            state.update(cx, |state, cx| state.set_search_query("world", true, cx));
            host.sync(&state, window, cx);
            assert!(input.read(cx).selected_range().is_empty());

            // The find key again, with the bar already up.
            state.update(cx, |state, cx| state.open_search(false, cx));
            host.sync(&state, window, cx);
            assert_eq!(
                input.read(cx).selected_range(),
                0..5,
                "the old query is offered for replacing"
            );

            // A query set by the app rather than by the field.
            state.update(cx, |state, cx| state.set_search_query("hello", true, cx));
            host.sync(&state, window, cx);
            assert_eq!(input.read(cx).value(), "hello");
        });
    }

    /// A frame that changed nothing must not rebuild the popovers.
    ///
    /// Sync runs every frame, so the change check has to be cheap and stable.
    /// It is keyed on a revision the engine bumps when it swaps the content;
    /// forgetting to bump, or comparing the content itself again, both show up
    /// here.
    #[gpui::test]
    fn unchanged_content_keeps_the_overlay_signature(cx: &mut gpui::TestAppContext) {
        cx.update(crate::init);
        let (probe, cx) = cx.add_window_view(|window, cx| OverlayProbe {
            state: cx.new(|cx| crate::input::EditorState::new(window, cx).language("sql")),
        });
        let state = probe.read_with(cx, |probe, _| probe.state.clone());

        cx.update(|window, cx| {
            state.update(cx, |state, cx| {
                state.present_completion_items(
                    0,
                    "f",
                    vec![CompletionItem {
                        label: "foo".into(),
                        ..Default::default()
                    }],
                    cx,
                );
            });

            let mut host = InputOverlayHost::new(state.clone(), window, cx);
            host.sync(&state, window, cx);
            let after_first = host.lsp.as_ref().unwrap().completion_signature.revision;

            // A second pass over untouched content.
            host.sync(&state, window, cx);
            assert_eq!(
                host.lsp.as_ref().unwrap().completion_signature.revision,
                after_first,
                "an unchanged frame must not bump the signature"
            );

            // Swapping the items must be noticed.
            state.update(cx, |state, cx| {
                state.present_completion_items(
                    0,
                    "f",
                    vec![CompletionItem {
                        label: "bar".into(),
                        ..Default::default()
                    }],
                    cx,
                );
            });
            host.sync(&state, window, cx);
            assert_ne!(
                host.lsp.as_ref().unwrap().completion_signature.revision,
                after_first,
                "new items must be noticed"
            );
        });
    }

    #[gpui::test]
    fn facade_materializes_all_base_overlay_sessions(cx: &mut gpui::TestAppContext) {
        cx.update(crate::init);
        let (probe, cx) = cx.add_window_view(|window, cx| OverlayProbe {
            state: cx.new(|cx| {
                crate::input::EditorState::new(window, cx)
                    .language("sql")
                    .searchable(true)
                    .replaceable(true)
            }),
        });
        let state = probe.read_with(cx, |probe, _| probe.state.clone());
        cx.update(|window, cx| {
            assert!(render_overlays(&state, window, cx).is_empty());
            assert!(state.read(cx).has_overlay_action_handler());
            state.update(cx, |state, cx| {
                state.set_value("foo bar foo", window, cx);
                state.set_selected_range(4..7, cx);
                state.open_search(true, cx);
                assert_eq!(state.search_session().query, "bar");
                assert!(state.search_session().replace_mode);
                state.present_completion_items(
                    0,
                    "f",
                    vec![CompletionItem {
                        label: "foo".into(),
                        ..Default::default()
                    }],
                    cx,
                );
                state.present_code_actions(
                    vec![CodeActionItem {
                        provider_id: SharedString::from("test"),
                        action: CodeAction {
                            title: "Fix".into(),
                            ..Default::default()
                        },
                    }],
                    cx,
                );
                state.present_hover(
                    0..1,
                    Hover {
                        contents: HoverContents::Scalar(MarkedString::String("docs".into())),
                        range: None,
                    },
                    cx,
                );
                state.present_diagnostic(DiagnosticEntry::default(), cx);
            });

            let mut host = InputOverlayHost::new(state.clone(), window, cx);
            let overlays = host.sync(&state, window, cx);
            assert!(overlays.search.is_some());
            assert_eq!(overlays.floating.len(), 4);
            assert_eq!(overlays.len(), 5);
            assert_eq!(render_overlays(&state, window, cx).len(), 5);
            assert!(
                cx.global::<InputOverlayRegistry<crate::input::EditorMode>>()
                    .hosts
                    .contains_key(&state.entity_id())
            );

            state.update(cx, |state, cx| {
                assert!(state.route_overlay_action(Box::new(super::super::Escape), window, cx));
                assert!(!state.completion_menu_state().open);
                state.dismiss_code_action_overlay(cx);
                state.close_search(cx);
                state.clear_hover_state(cx);
                state.clear_diagnostic_popover(cx);
            });
            assert!(host.sync(&state, window, cx).is_empty());
            assert!(render_overlays(&state, window, cx).is_empty());
            assert!(
                !cx.global::<InputOverlayRegistry<crate::input::EditorMode>>()
                    .hosts
                    .contains_key(&state.entity_id())
            );
        });

        let dropped_owner = cx.update(|window, cx| {
            let ephemeral = cx.new(|cx| {
                crate::input::EditorState::new(window, cx)
                    .language("sql")
                    .searchable(true)
            });
            ephemeral.update(cx, |state, cx| state.open_search(false, cx));
            assert_eq!(render_overlays(&ephemeral, window, cx).len(), 1);
            ephemeral.update(cx, |state, cx| state.close_search(cx));
            assert!(render_overlays(&ephemeral, window, cx).is_empty());
            assert!(
                !cx.global::<InputOverlayRegistry<crate::input::EditorMode>>()
                    .hosts
                    .contains_key(&ephemeral.entity_id())
            );
            let owner = ephemeral.downgrade();
            drop(ephemeral);
            owner
        });
        cx.run_until_parked();
        assert!(dropped_owner.upgrade().is_none());
    }
}
