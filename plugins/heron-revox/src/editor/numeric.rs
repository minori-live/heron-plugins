//! Focus the native numeric field once when opened, without an Iced Task executor.

use truce_iced::{
    Message,
    iced::{
        Element, Renderer, Theme,
        widget::{
            TextInput,
            core::{
                self, Widget,
                widget::{Tree, tree},
            },
            text_input,
        },
    },
};

use super::RevoxMessage;

type UiMessage = Message<RevoxMessage>;
type InputState = text_input::State<<Renderer as core::text::Renderer>::Paragraph>;

pub(super) fn focus_on_open<'a>(
    input: TextInput<'a, UiMessage>,
    generation: u64,
) -> Element<'a, UiMessage> {
    Element::new(FocusOnOpen {
        input: input.into(),
        generation,
    })
}

struct FocusOnOpen<'a> {
    input: Element<'a, UiMessage>,
    generation: u64,
}

struct FocusState {
    generation: u64,
}

impl Widget<UiMessage, Theme, Renderer> for FocusOnOpen<'_> {
    fn size(&self) -> core::Size<core::Length> {
        self.input.as_widget().size()
    }
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<FocusState>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(FocusState {
            generation: self.generation,
        })
    }
    fn children(&self) -> Vec<Tree> {
        let mut input = Tree::new(&self.input);
        focus(&mut input);
        vec![input]
    }
    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[&self.input]);
        let state = tree.state.downcast_mut::<FocusState>();
        if state.generation != self.generation {
            state.generation = self.generation;
            focus(&mut tree.children[0]);
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &core::layout::Limits,
    ) -> core::layout::Node {
        self.input
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &core::renderer::Style,
        layout: core::Layout<'_>,
        cursor: core::mouse::Cursor,
        viewport: &core::Rectangle,
    ) {
        self.input.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: core::Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn core::widget::Operation,
    ) {
        self.input
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &core::Event,
        layout: core::Layout<'_>,
        cursor: core::mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn core::Clipboard,
        shell: &mut core::Shell<'_, UiMessage>,
        viewport: &core::Rectangle,
    ) {
        self.input.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: core::Layout<'_>,
        cursor: core::mouse::Cursor,
        viewport: &core::Rectangle,
        renderer: &Renderer,
    ) -> core::mouse::Interaction {
        self.input.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }
    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut Tree,
        layout: core::Layout<'a>,
        renderer: &Renderer,
        viewport: &core::Rectangle,
        translation: core::Vector,
    ) -> Option<core::overlay::Element<'a, UiMessage, Theme, Renderer>> {
        self.input.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

fn focus(tree: &mut Tree) {
    // The only child is the TextInput passed to focus_on_open. Its public state
    // keeps native selection, keyboard, clipboard and IME behavior.
    let input = tree.state.downcast_mut::<InputState>();
    input.focus();
    input.select_all();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_numeric_edit_focuses_and_selects_once_without_a_task_executor() {
        let make = |generation| {
            focus_on_open(
                text_input("", "-12").on_input(|_| Message::Plugin(RevoxMessage::Unfocused)),
                generation,
            )
        };
        let input = make(1);
        let mut tree = Tree::new(&input);
        let value = text_input::Value::new("-12");
        let state = tree.children[0].state.downcast_mut::<InputState>();
        assert!(state.is_focused());
        assert_eq!(state.cursor().selection(&value), Some((0, 3)));
        // Ordinary view refresh must preserve a user's new caret/selection.
        state.move_cursor_to(1);
        tree.diff(make(1));
        assert_eq!(
            tree.children[0]
                .state
                .downcast_ref::<InputState>()
                .cursor()
                .selection(&value),
            None
        );
        tree.diff(make(2));
        assert_eq!(
            tree.children[0]
                .state
                .downcast_ref::<InputState>()
                .cursor()
                .selection(&value),
            Some((0, 3))
        );
    }
}
