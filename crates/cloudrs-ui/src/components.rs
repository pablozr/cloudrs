//! Primitives shared by every screen.
//!
//! Each takes the [`Theme`] so it never reaches for a raw value. Interactive
//! primitives return a `Stateful<Div>` so callers attach `on_click`.

use gpui::prelude::*;
use gpui::{
    Bounds, Div, ElementId, Hsla, PathBuilder, Pixels, SharedString, Stateful, canvas, div, fill,
    linear_color_stop, linear_gradient, point, px,
};

use crate::Theme;
use crate::tokens::{self, radius, space, typography};

/// Visual weight of a [`button`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    /// The one main action of a region (accent fill).
    Primary,
    /// A raised surface with a border.
    Secondary,
    /// An accent outline.
    Outline,
    /// Text only, until hovered.
    Ghost,
}

/// A text button.
pub fn button(
    theme: &Theme,
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    kind: ButtonKind,
) -> Stateful<Div> {
    let c = theme.colors;
    let base = theme
        .text(div(), typography::BODY)
        .id(id)
        .flex()
        .items_center()
        .gap(space::S2)
        .px(space::S4)
        .py(space::S2)
        .rounded(radius::M)
        .border_1()
        .border_color(gpui::transparent_black())
        .cursor_pointer()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .child(label.into());
    match kind {
        ButtonKind::Primary => base
            .bg(c.accent)
            .text_color(c.on_accent)
            .hover(move |s| s.bg(c.accent_hover)),
        ButtonKind::Secondary => base
            .bg(c.surface_raised)
            .border_color(c.line_strong)
            .text_color(c.text)
            .hover(move |s| s.bg(c.surface_hover)),
        ButtonKind::Outline => base
            .border_color(c.accent.opacity(0.45))
            .text_color(c.accent_hover)
            .hover(move |s| s.bg(c.accent_soft)),
        ButtonKind::Ghost => base
            .text_color(c.text_muted)
            .hover(move |s| s.bg(c.surface_raised).text_color(c.text)),
    }
}

/// The round play/pause button with the accent gradient.
pub fn play_button(
    theme: &Theme,
    id: impl Into<ElementId>,
    playing: bool,
    size: Pixels,
) -> Stateful<Div> {
    let [from, _, to] = tokens::accent_gradient();
    let glyph = theme.colors.on_accent;
    let glow = theme.colors.accent_glow;
    div()
        .id(id)
        .size(size)
        .flex_none()
        .rounded(radius::FULL)
        .cursor_pointer()
        .bg(linear_gradient(
            135.0,
            linear_color_stop(from, 0.0),
            linear_color_stop(to, 1.0),
        ))
        .shadow(vec![gpui::BoxShadow {
            color: glow,
            offset: point(px(0.0), px(6.0)),
            blur_radius: px(20.0),
            spread_radius: px(0.0),
            inset: false,
        }])
        .hover(|s| s.opacity(0.92))
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, (), window, _| paint_play_glyph(bounds, playing, glyph, window),
            )
            .size_full(),
        )
}

fn paint_play_glyph(bounds: Bounds<Pixels>, playing: bool, color: Hsla, window: &mut gpui::Window) {
    let side = bounds.size.width.min(bounds.size.height);
    let unit = side / 24.0;
    let origin = bounds.origin;
    let at = |x: f32, y: f32| point(origin.x + unit * x, origin.y + unit * y);
    if playing {
        for x in [7.0, 13.5] {
            let bar = Bounds::from_corners(at(x, 7.0), at(x + 3.5, 17.0));
            window.paint_quad(fill(bar, color).corner_radii(unit));
        }
    } else {
        let mut path = PathBuilder::fill();
        path.move_to(at(9.0, 6.5));
        path.line_to(at(18.0, 12.0));
        path.line_to(at(9.0, 17.5));
        path.close();
        if let Ok(path) = path.build() {
            window.paint_path(path, color);
        }
    }
}

/// SoundCloud-style waveform: bars drawn from `samples` (0..=1), filled with
/// the accent up to `progress` (0..=1).
/// Fills its parent; size the parent.
pub fn waveform(theme: &Theme, samples: Vec<f32>, progress: f32) -> gpui::Canvas<()> {
    let played = theme.colors.accent;
    let rest = theme.colors.surface_hover;
    canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            if samples.is_empty() {
                return;
            }
            let gap = px(2.0);
            let count = samples.len() as f32;
            let bar = ((bounds.size.width - gap * (count - 1.0)) / count).max(px(1.0));
            let height = bounds.size.height;
            for (i, sample) in samples.iter().enumerate() {
                let x = bounds.origin.x + (bar + gap) * i as f32;
                let h = (height * sample.clamp(0.08, 1.0)).max(px(2.0));
                let y = bounds.origin.y + (height - h) / 2.0;
                let color = if (i as f32 + 0.5) / count <= progress {
                    played
                } else {
                    rest
                };
                let rect = Bounds::from_corners(point(x, y), point(x + bar, y + h));
                window.paint_quad(fill(rect, color).corner_radii(bar / 2.0));
            }
        },
    )
    .size_full()
}

/// A small status label (`AAC 160k`, `GO+`, `30s preview`).
pub fn badge(theme: &Theme, label: impl Into<SharedString>, color: Hsla) -> Div {
    theme
        .text(div(), typography::LABEL)
        .px(space::S2)
        .py(px(2.0))
        .rounded(radius::S)
        .bg(color.opacity(0.14))
        .text_color(theme.readable(color))
        .child(label.into())
}

/// A filter pill; the selected one is inverted.
pub fn pill(
    theme: &Theme,
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
) -> Stateful<Div> {
    let c = theme.colors;
    let pill = theme
        .text(div(), typography::BODY_MUTED)
        .id(id)
        .px(space::S4)
        .py(px(6.0))
        .rounded(radius::FULL)
        .border_1()
        .cursor_pointer()
        .child(label.into());
    if selected {
        pill.bg(c.text).border_color(c.text).text_color(c.canvas)
    } else {
        pill.border_color(c.line)
            .text_color(c.text_muted)
            .hover(move |s| s.bg(c.surface_raised).text_color(c.text))
    }
}
