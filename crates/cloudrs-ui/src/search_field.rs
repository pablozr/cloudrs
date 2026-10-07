//! A single-line, editable text field with OS text input, selection,
//! clipboard and IME. GPUI ships no text input, so this owns the editing.
//!
//! Adapted from xemnas `ui/search_field.rs` (MIT, same maintainer); see
//! `NOTICE`. The multi-line and masked modes were left out.

use std::ops::Range;

use gpui::prelude::*;
use gpui::{
    App, Bounds, ClipboardItem, Context, Element, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId, KeyBinding,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point,
    Render, Role, ShapedLine, SharedString, Style, TextAlign, TextRun, UTF16Selection,
    UnderlineStyle, Window, actions, div, fill, point, px, relative, size,
};

use crate::Theme;
use crate::components::{Icon, icon};
use crate::search_edit::SearchEdit;
use crate::tokens::{radius, size, space, typography};

actions!(
    search_field,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        Paste,
        Copy,
        Cut,
        Clear
    ]
);

/// The key context the field's bindings live in.
const CONTEXT: &str = "SearchField";

/// Registers the field's key bindings. Call once at startup.
pub fn bind_keys(cx: &mut App) {
    let context = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, context),
        KeyBinding::new("delete", Delete, context),
        KeyBinding::new("left", Left, context),
        KeyBinding::new("right", Right, context),
        KeyBinding::new("shift-left", SelectLeft, context),
        KeyBinding::new("shift-right", SelectRight, context),
        KeyBinding::new("home", Home, context),
        KeyBinding::new("end", End, context),
        KeyBinding::new("escape", Clear, context),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-a", SelectAll, context),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-v", Paste, context),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-c", Copy, context),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-x", Cut, context),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-a", SelectAll, context),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-v", Paste, context),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-c", Copy, context),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-x", Cut, context),
    ]);
}

/// Emitted whenever the text changes.
pub struct SearchChanged(pub String);

pub struct SearchField {
    placeholder: SharedString,
    /// Shown on the right while unfocused (e.g. the shortcut).
    hint: SharedString,
    focus: FocusHandle,
    edit: SearchEdit,
    layout: Option<ShapedLine>,
    bounds: Option<Bounds<Pixels>>,
    scroll_x: Pixels,
    selecting: bool,
}

impl EventEmitter<SearchChanged> for SearchField {}

impl SearchField {
    pub fn new(
        placeholder: impl Into<SharedString>,
        hint: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            placeholder: placeholder.into(),
            hint: hint.into(),
            focus: cx.focus_handle().tab_stop(true),
            edit: SearchEdit::default(),
            layout: None,
            bounds: None,
            scroll_x: px(0.0),
            selecting: false,
        }
    }

    pub fn value(&self) -> &str {
        &self.edit.text
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        self.layout = None;
        cx.emit(SearchChanged(self.edit.text.clone()));
        cx.notify();
    }

    fn index_at(&self, position: Point<Pixels>) -> usize {
        match (self.layout.as_ref(), self.bounds.as_ref()) {
            (Some(line), Some(bounds)) => {
                line.closest_index_for_x(position.x - bounds.left() + self.scroll_x)
            }
            _ => 0,
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        self.selecting = true;
        let index = self.index_at(event.position);
        if event.modifiers.shift {
            self.edit.select_to(index);
        } else {
            self.edit.move_to(index);
        }
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.selecting {
            let index = self.index_at(event.position);
            self.edit.select_to(index);
            cx.notify();
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.selecting = false;
    }

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        self.edit.backspace();
        self.changed(cx);
    }

    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        self.edit.delete();
        self.changed(cx);
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        let offset = if self.edit.selection.is_empty() {
            self.edit.previous_boundary(self.edit.caret())
        } else {
            self.edit.selection.start
        };
        self.edit.move_to(offset);
        cx.notify();
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        let offset = if self.edit.selection.is_empty() {
            self.edit.next_boundary(self.edit.caret())
        } else {
            self.edit.selection.end
        };
        self.edit.move_to(offset);
        cx.notify();
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        let offset = self.edit.previous_boundary(self.edit.caret());
        self.edit.select_to(offset);
        cx.notify();
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        let offset = self.edit.next_boundary(self.edit.caret());
        self.edit.select_to(offset);
        cx.notify();
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.edit.move_to(0);
        self.edit.select_to(self.edit.text.len());
        cx.notify();
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.edit.move_to(0);
        cx.notify();
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.edit.move_to(self.edit.text.len());
        cx.notify();
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.edit.replace(None, &text);
            self.changed(cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.edit.selection.is_empty() {
            let selected = self.edit.text[self.edit.selection.clone()].to_string();
            cx.write_to_clipboard(ClipboardItem::new_string(selected));
        }
    }

    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if !self.edit.selection.is_empty() {
            let selected = self.edit.text[self.edit.selection.clone()].to_string();
            cx.write_to_clipboard(ClipboardItem::new_string(selected));
            self.edit.replace(None, "");
            self.changed(cx);
        }
    }

    fn clear(&mut self, _: &Clear, _: &mut Window, cx: &mut Context<Self>) {
        self.edit = SearchEdit::default();
        self.changed(cx);
    }
}

impl Focusable for SearchField {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EntityInputHandler for SearchField {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let start = self.edit.byte_offset(range.start);
        let end = self.edit.byte_offset(range.end);
        *actual = Some(self.edit.utf16_offset(start)..self.edit.utf16_offset(end));
        Some(self.edit.text.get(start..end)?.to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.edit.utf16_offset(self.edit.selection.start)
                ..self.edit.utf16_offset(self.edit.selection.end),
            reversed: self.edit.reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.edit
            .marked
            .as_ref()
            .map(|range| self.edit.utf16_offset(range.start)..self.edit.utf16_offset(range.end))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.edit.marked = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range.map(|r| self.edit.byte_offset(r.start)..self.edit.byte_offset(r.end));
        self.edit.replace(range, text);
        self.changed(cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range.map(|r| self.edit.byte_offset(r.start)..self.edit.byte_offset(r.end));
        self.edit.replace_and_mark(range, text, selected);
        self.changed(cx);
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let line = self.layout.as_ref()?;
        let start = self.edit.byte_offset(range.start);
        let end = self.edit.byte_offset(range.end);
        Some(Bounds::from_corners(
            point(
                bounds.left() + line.x_for_index(start) - self.scroll_x,
                bounds.top(),
            ),
            point(
                bounds.left() + line.x_for_index(end) - self.scroll_x,
                bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        position: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.edit.utf16_offset(self.index_at(position)))
    }

    fn set_selected_text_range(
        &mut self,
        range: Range<usize>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.edit.selection = self.edit.byte_offset(range.start)..self.edit.byte_offset(range.end);
        self.edit.reversed = false;
        cx.notify();
    }
}

/// The painted text line: shaping, selection, caret and the OS input handler.
struct TextElement {
    input: Entity<SearchField>,
}

struct TextPaint {
    line: ShapedLine,
    selection: Option<PaintQuad>,
    caret: Option<PaintQuad>,
    scroll_x: Pixels,
}

impl IntoElement for TextElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = TextPaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> TextPaint {
        let theme = Theme::of(cx);
        let input = self.input.read(cx);
        let focused = input.focus.is_focused(window);
        let empty = input.edit.text.is_empty();
        let content: SharedString = if empty && !focused {
            input.placeholder.clone()
        } else {
            input.edit.text.clone().into()
        };
        let style = window.text_style();
        let run = TextRun {
            len: content.len(),
            font: style.font(),
            color: if empty {
                theme.colors.text_subtle
            } else {
                theme.colors.text
            },
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = match &input.edit.marked {
            Some(marked) => [
                TextRun {
                    len: marked.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked.end - marked.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: content.len() - marked.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect(),
            None => vec![run],
        };
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(content, font_size, &runs, None);
        let caret_x = line.x_for_index(input.edit.caret());
        let scroll_x = if focused {
            (caret_x - bounds.size.width + px(4.0)).max(px(0.0))
        } else {
            px(0.0)
        };
        let origin_x = bounds.left() - scroll_x;
        let selection = (!input.edit.selection.is_empty()).then(|| {
            fill(
                Bounds::from_corners(
                    point(
                        origin_x + line.x_for_index(input.edit.selection.start),
                        bounds.top(),
                    ),
                    point(
                        origin_x + line.x_for_index(input.edit.selection.end),
                        bounds.bottom(),
                    ),
                ),
                theme.colors.accent_soft,
            )
        });
        let caret = (focused && input.edit.selection.is_empty()).then(|| {
            fill(
                Bounds::new(
                    point(origin_x + caret_x, bounds.top()),
                    size(px(1.5), bounds.size.height),
                ),
                theme.colors.accent,
            )
        });
        TextPaint {
            line,
            selection,
            caret,
            scroll_x,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        paint: &mut TextPaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.input.read(cx).focus.clone();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        if let Some(selection) = paint.selection.take() {
            window.paint_quad(selection);
        }
        let origin = point(bounds.left() - paint.scroll_x, bounds.top());
        if let Err(error) = paint.line.paint(
            origin,
            window.line_height(),
            TextAlign::Left,
            None,
            window,
            cx,
        ) {
            tracing::warn!(%error, "could not paint the search text");
        }
        if let Some(caret) = paint.caret.take() {
            window.paint_quad(caret);
        }
        self.input.update(cx, |input, _| {
            input.layout = Some(paint.line.clone());
            input.bounds = Some(bounds);
            input.scroll_x = paint.scroll_x;
        });
    }
}

impl Render for SearchField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let c = theme.colors;
        let focused = self.focus.is_focused(window);
        theme
            .text(div(), typography::BODY)
            .id("search-field")
            .w_full()
            .h(px(38.0))
            .px(space::S4)
            .flex()
            .items_center()
            .gap(space::S2)
            .rounded(radius::FULL)
            .border_1()
            .border_color(if focused { c.accent } else { c.line })
            .bg(c.surface)
            .when(!focused, |field| {
                field.hover(move |s| s.border_color(c.line_strong))
            })
            .track_focus(&self.focus)
            .key_context(CONTEXT)
            .role(Role::TextInput)
            .aria_label(self.placeholder.clone())
            .cursor_text()
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::clear))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .child(icon(Icon::Search, size::ICON_S, c.text_subtle))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .child(TextElement { input: cx.entity() }),
            )
            .when(!focused && !self.hint.is_empty(), |field| {
                field.child(
                    theme
                        .text(div(), typography::MONO)
                        .px(space::S2)
                        .rounded(radius::S)
                        .border_1()
                        .border_color(c.line_strong)
                        .text_color(c.text_subtle)
                        .child(self.hint.clone()),
                )
            })
    }
}
