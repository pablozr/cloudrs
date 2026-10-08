//! The command palette surface (ADR 0018): an overlay with a search field on
//! top and a list of rows below. It draws; the app owns the items, the filter
//! and what a row does.

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Div, ElementId, MouseButton, MouseDownEvent, Role, ScrollHandle, SharedString,
    Stateful, Window, div, px,
};

use crate::components::{Icon, menu_item};
use crate::tokens::{self, radius, size, space, typography};
use crate::{Theme, motion};

/// The key context of the open palette, for the app's key bindings.
pub const CONTEXT: &str = "CommandPalette";

/// Whether `label` fits what was typed: every word of `query` is somewhere in
/// the label, ignoring case. An empty query fits everything.
pub fn matches(query: &str, label: &str) -> bool {
    let label = label.to_lowercase();
    query
        .split_whitespace()
        .all(|word| label.contains(&word.to_lowercase()))
}

/// The palette over a dimmed window, with the panel near the top. `field` is
/// the search field, `rows` are [`palette_row`]s, and `empty` replaces them
/// with a line of text. A click on the dim area calls `on_dismiss`. It covers
/// its positioned parent.
#[allow(clippy::too_many_arguments)]
pub fn palette(
    theme: &Theme,
    id: impl Into<ElementId>,
    label: SharedString,
    field: AnyElement,
    rows: Vec<AnyElement>,
    empty: Option<SharedString>,
    scroll: &ScrollHandle,
    on_dismiss: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let c = theme.colors;
    let list = div()
        .id("palette-list")
        .role(Role::ListBox)
        .max_h(size::PALETTE_MAX_HEIGHT)
        .overflow_y_scroll()
        .track_scroll(scroll)
        .flex()
        .flex_col()
        .p(space::S1)
        .children(rows)
        .when_some(empty, |list, text| {
            list.child(
                theme
                    .text(div(), typography::BODY_MUTED)
                    .px(space::S3)
                    .py(space::S3)
                    .text_color(c.text_muted)
                    .child(text),
            )
        });
    div()
        .id(id)
        .absolute()
        .top(px(0.0))
        .left(px(0.0))
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .pt(space::S8)
        .bg(c.canvas.opacity(0.72))
        .occlude()
        .key_context(CONTEXT)
        .role(Role::Dialog)
        .aria_label(label)
        .on_mouse_down(MouseButton::Left, on_dismiss)
        .child(motion::pop_in(
            "palette-panel",
            div()
                .occlude()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .w(size::PALETTE_WIDTH)
                .max_w_full()
                .flex()
                .flex_col()
                .rounded(radius::XL)
                .border_1()
                .border_color(c.line_strong)
                .bg(c.surface)
                .shadow(tokens::floating_shadow())
                .child(div().p(space::S2).child(field))
                .child(div().h(px(1.0)).bg(c.line))
                .child(list),
        ))
}

/// One row of the palette: icon, label and, on the right, the key that does
/// the same. The selected row (moved with the arrow keys) has the accent
/// border, the keyboard's focus. Callers add `on_click` and `aria_label`.
pub fn palette_row(
    theme: &Theme,
    id: impl Into<ElementId>,
    glyph: Icon,
    label: impl Into<SharedString>,
    keys: Option<&'static str>,
    selected: bool,
) -> Stateful<Div> {
    let c = theme.colors;
    menu_item(theme, id, glyph, label)
        .role(Role::ListBoxOption)
        .when(selected, |row| {
            row.bg(c.surface_hover).border_color(c.accent)
        })
        .when_some(keys, |row, keys| {
            row.child(
                theme
                    .text(div(), typography::MONO)
                    .px(space::S2)
                    .rounded(radius::S)
                    .border_1()
                    .border_color(c.line_strong)
                    .text_color(c.text_subtle)
                    .child(keys),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_query_matches_everything() {
        assert!(matches("", "Settings"));
        assert!(matches("   ", "Settings"));
    }

    #[test]
    fn every_word_must_appear_in_any_order_ignoring_case() {
        assert!(matches("SET", "Settings"));
        assert!(matches("theme dark", "Theme: dark"));
        assert!(matches("dark theme", "Theme: dark"));
        assert!(!matches("theme light", "Theme: dark"));
        assert!(!matches("xyz", "Settings"));
    }
}
