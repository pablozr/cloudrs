//! Primitives shared by every screen.
//!
//! Each takes the [`Theme`] so it never reaches for a raw value. Interactive
//! primitives return a `Stateful<Div>` so callers attach `on_click`,
//! `aria_label` and `tooltip`.

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    Animation, AnimationExt, AnyElement, AnyView, App, Bounds, ClickEvent, Context, Div, ElementId,
    Hsla, MouseButton, MouseDownEvent, ObjectFit, PathBuilder, Pixels, Render, Role, SharedString,
    Stateful, Window, canvas, div, fill, img, linear_color_stop, linear_gradient, point, px,
};

use crate::tokens::{self, radius, size, space, typography};
use crate::{Theme, motion};

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
        .tab_index(0)
        .focus_visible(move |s| s.border_color(c.accent))
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
    let ring = theme.colors.accent_hover;
    div()
        .id(id)
        .size(size)
        .flex_none()
        .rounded(radius::FULL)
        .tab_index(0)
        .focus_visible(move |s| s.shadow(tokens::focus_ring(ring)))
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

fn paint_play_glyph(bounds: Bounds<Pixels>, playing: bool, color: Hsla, window: &mut Window) {
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

/// Glyphs of the icon buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Previous,
    Next,
    Shuffle,
    Repeat,
    /// Repeat with a mark: the same track plays again.
    RepeatOne,
    Queue,
}

fn paint_icon(bounds: Bounds<Pixels>, icon: Icon, color: Hsla, window: &mut Window) {
    let side = bounds.size.width.min(bounds.size.height);
    let unit = side / 24.0;
    let origin = bounds.origin;
    let at = |(x, y): (f32, f32)| point(origin.x + unit * x, origin.y + unit * y);
    let stroke = |window: &mut Window, points: &[(f32, f32)]| {
        let mut path = PathBuilder::stroke(unit * 2.0);
        path.move_to(at(points[0]));
        for point in &points[1..] {
            path.line_to(at(*point));
        }
        if let Ok(path) = path.build() {
            window.paint_path(path, color);
        }
    };
    let triangle = |window: &mut Window, a: (f32, f32), b: (f32, f32), c: (f32, f32)| {
        let mut path = PathBuilder::fill();
        path.move_to(at(a));
        path.line_to(at(b));
        path.line_to(at(c));
        path.close();
        if let Ok(path) = path.build() {
            window.paint_path(path, color);
        }
    };
    let bar = |window: &mut Window, from: (f32, f32), to: (f32, f32)| {
        let rect = Bounds::from_corners(at(from), at(to));
        window.paint_quad(fill(rect, color).corner_radii(unit));
    };
    match icon {
        Icon::Previous => {
            bar(window, (6.0, 6.0), (8.5, 18.0));
            triangle(window, (18.0, 6.0), (18.0, 18.0), (9.5, 12.0));
        }
        Icon::Next => {
            bar(window, (15.5, 6.0), (18.0, 18.0));
            triangle(window, (6.0, 6.0), (6.0, 18.0), (14.5, 12.0));
        }
        Icon::Queue => {
            bar(window, (4.0, 6.0), (18.0, 8.0));
            bar(window, (4.0, 11.0), (18.0, 13.0));
            bar(window, (4.0, 16.0), (11.0, 18.0));
            triangle(window, (15.0, 14.0), (21.0, 17.0), (15.0, 20.0));
        }
        Icon::Shuffle => {
            stroke(
                window,
                &[(3.0, 7.0), (8.0, 7.0), (15.0, 17.0), (18.0, 17.0)],
            );
            stroke(
                window,
                &[(3.0, 17.0), (8.0, 17.0), (15.0, 7.0), (18.0, 7.0)],
            );
            triangle(window, (17.0, 4.0), (22.0, 7.0), (17.0, 10.0));
            triangle(window, (17.0, 14.0), (22.0, 17.0), (17.0, 20.0));
        }
        Icon::Repeat | Icon::RepeatOne => {
            stroke(window, &[(4.0, 12.0), (4.0, 8.0), (17.0, 8.0)]);
            stroke(window, &[(20.0, 12.0), (20.0, 16.0), (7.0, 16.0)]);
            triangle(window, (16.0, 4.5), (21.0, 8.0), (16.0, 11.5));
            triangle(window, (8.0, 12.5), (3.0, 16.0), (8.0, 19.5));
            if icon == Icon::RepeatOne {
                bar(window, (11.0, 10.0), (13.0, 14.0));
            }
        }
    }
}

/// A round icon-only button. Callers add the `aria_label` and a `tooltip`.
/// `active` marks a toggle that is on.
pub fn icon_button(
    theme: &Theme,
    id: impl Into<ElementId>,
    icon: Icon,
    active: bool,
) -> Stateful<Div> {
    let c = theme.colors;
    let color = if active { c.accent } else { c.text_muted };
    div()
        .id(id)
        .flex_none()
        .size(size::ICON_BUTTON)
        .p(space::S2)
        .rounded(radius::FULL)
        .tab_index(0)
        .focus_visible(move |s| s.shadow(tokens::focus_ring(c.accent)))
        .cursor_pointer()
        .hover(move |s| s.bg(c.surface_hover))
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, (), window, _| paint_icon(bounds, icon, color, window),
            )
            .size_full(),
        )
}

/// A small text action shown on a row while it is hovered ("Play next").
/// It handles its own click, so the row's click does not fire.
pub fn row_action(
    theme: &Theme,
    id: impl Into<ElementId>,
    label: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let c = theme.colors;
    theme
        .text(div(), typography::BODY_MUTED)
        .id(id)
        .px(space::S2)
        .py(space::S1)
        .rounded(radius::S)
        .text_color(c.text_muted)
        .cursor_pointer()
        .tab_index(0)
        .focus_visible(move |s| s.shadow(tokens::focus_ring(c.accent)))
        .hover(move |s| s.bg(c.surface_hover).text_color(c.text))
        .aria_label(label)
        .child(label)
        .on_click(move |event, window, cx| {
            cx.stop_propagation();
            on_click(event, window, cx);
        })
}

/// What follows the pointer while a row is dragged: lifted (shadow) with an
/// accent outline.
pub fn drag_preview(theme: &Theme, title: SharedString, artist: SharedString) -> Div {
    let c = theme.colors;
    div()
        .w(size::DRAG_PREVIEW_WIDTH)
        .flex()
        .flex_col()
        .px(space::S3)
        .py(space::S2)
        .rounded(radius::M)
        .border_1()
        .border_color(c.accent)
        .bg(c.surface_raised)
        .shadow(tokens::floating_shadow())
        .child(
            theme
                .text(div(), typography::BODY)
                .truncate()
                .text_color(c.text)
                .child(title),
        )
        .child(
            theme
                .text(div(), typography::BODY_MUTED)
                .truncate()
                .text_color(c.text_muted)
                .child(artist),
        )
}

/// Horizontal position of `x` inside `left..left + width`, clamped to 0..=1.
fn fraction_in(x: f32, left: f32, width: f32) -> f32 {
    if width <= 0.0 {
        return 0.0;
    }
    ((x - left) / width).clamp(0.0, 1.0)
}

/// Called with the pointer's fraction while it is over a strip, `None` on leave.
type HoverHandler = Rc<dyn Fn(Option<f32>, &mut Window, &mut App)>;

/// A focusable strip that reports the pointer's horizontal fraction (0..=1) on
/// press and, with `drag`, while the left button stays down. `on_hover`, when
/// given, follows the pointer over the strip. `paint` draws it.
fn scrubber(
    id: impl Into<ElementId>,
    ring: Hsla,
    drag: bool,
    on_hover: Option<HoverHandler>,
    paint: impl Fn(Bounds<Pixels>, &mut Window) + 'static,
    on_change: impl Fn(f32, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    // Window coordinates of the last layout, to turn pointer events into fractions.
    let bounds = Rc::new(Cell::new(Bounds::<Pixels>::default()));
    let on_change = Rc::new(on_change);
    let fraction = {
        let bounds = bounds.clone();
        move |x: Pixels| {
            let b = bounds.get();
            fraction_in(x.into(), b.left().into(), b.size.width.into())
        }
    };
    let on_press = {
        let (on_change, fraction) = (on_change.clone(), fraction.clone());
        move |event: &MouseDownEvent, window: &mut Window, cx: &mut App| {
            on_change(fraction(event.position.x), window, cx);
        }
    };
    let strip = div()
        .id(id)
        .size_full()
        .rounded(radius::S)
        .tab_index(0)
        .focus_visible(move |s| s.shadow(tokens::focus_ring(ring)))
        .cursor_pointer()
        .role(Role::Slider)
        .on_mouse_down(MouseButton::Left, on_press)
        .child(
            canvas(
                move |b, _, _| bounds.set(b),
                move |b, (), window, _| paint(b, window),
            )
            .size_full(),
        );
    if !drag && on_hover.is_none() {
        return strip;
    }
    let leave = on_hover.clone();
    strip
        .on_mouse_move(move |event, window, cx| {
            let at = fraction(event.position.x);
            if let Some(on_hover) = &on_hover {
                on_hover(Some(at), window, cx);
            }
            if drag && event.dragging() {
                on_change(at, window, cx);
            }
        })
        .on_hover(move |hovered, window, cx| {
            if let (false, Some(on_hover)) = (*hovered, &leave) {
                on_hover(None, window, cx);
            }
        })
}

/// How a waveform bar is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BarKind {
    Played,
    /// Between the progress and the hover: what a click would change.
    Preview,
    Rest,
}

/// Kind of the bar centred at `at` (0..=1) for the current `progress` and the
/// pointer's `hover` fraction. Hovering ahead previews the bars a click would
/// play; hovering behind previews the played bars it would give up.
fn bar_kind(at: f32, progress: f32, hover: Option<f32>) -> BarKind {
    let (kept, edge) = match hover {
        Some(h) if h < progress => (h, progress),
        Some(h) => (progress, h),
        None => (progress, progress),
    };
    if at <= kept {
        BarKind::Played
    } else if at <= edge {
        BarKind::Preview
    } else {
        BarKind::Rest
    }
}

/// SoundCloud-style waveform: bars drawn from `samples` (0..=1), filled with
/// the accent up to `progress` (0..=1). A click calls `on_seek` with the
/// fraction clicked. `on_hover` follows the pointer (`None` on leave); the
/// caller keeps the fraction and passes it back as `hover`, so the bars a click
/// would change show a muted tint. Fills its parent (size the parent); the
/// caller adds the `aria_label`.
pub fn waveform(
    theme: &Theme,
    id: impl Into<ElementId>,
    samples: Arc<[f32]>,
    progress: f32,
    hover: Option<f32>,
    on_hover: impl Fn(Option<f32>, &mut Window, &mut App) + 'static,
    on_seek: impl Fn(f32, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let played = theme.colors.accent;
    let preview = theme.colors.accent_preview;
    let rest = theme.colors.surface_hover;
    // Click only: a seek restarts the HLS segment, so dragging would flood the core.
    scrubber(
        id,
        theme.colors.accent,
        false,
        Some(Rc::new(on_hover)),
        move |bounds, window| {
            if samples.is_empty() {
                return;
            }
            // Bars share the width evenly; on a narrow strip the gap shrinks
            // instead of the bars running past the end.
            let count = samples.len() as f32;
            let pitch = bounds.size.width / count;
            let gap = (pitch * tokens::WAVEFORM_GAP_RATIO).min(size::WAVEFORM_GAP);
            let bar = (pitch - gap).max(size::WAVEFORM_BAR_MIN);
            let height = bounds.size.height;
            for (i, sample) in samples.iter().enumerate() {
                let x = bounds.origin.x + pitch * i as f32;
                let h = (height * sample.clamp(0.08, 1.0)).max(px(2.0));
                let y = bounds.origin.y + (height - h) / 2.0;
                let color = match bar_kind((i as f32 + 0.5) / count, progress, hover) {
                    BarKind::Played => played,
                    BarKind::Preview => preview,
                    BarKind::Rest => rest,
                };
                let rect = Bounds::from_corners(point(x, y), point(x + bar, y + h));
                window.paint_quad(fill(rect, color).corner_radii(bar / 2.0));
            }
        },
        on_seek,
    )
}

/// A horizontal slider (`value` 0..=1) with click and drag. Fills its parent's
/// width at [`size::SLIDER_HEIGHT`]; the caller adds the `aria_label`.
pub fn slider(
    theme: &Theme,
    id: impl Into<ElementId>,
    value: f32,
    on_change: impl Fn(f32, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let c = theme.colors;
    let value = value.clamp(0.0, 1.0);
    scrubber(
        id,
        c.accent,
        true,
        None,
        move |bounds, window| {
            let track_h = size::SLIDER_TRACK;
            let thumb = size::SLIDER_THUMB;
            let mid = bounds.origin.y + bounds.size.height / 2.0;
            // The thumb stays inside the strip at both ends.
            let span = (bounds.size.width - thumb).max(px(0.0));
            let thumb_x = bounds.origin.x + span * value;
            let track = Bounds::from_corners(
                point(bounds.origin.x, mid - track_h / 2.0),
                point(bounds.origin.x + bounds.size.width, mid + track_h / 2.0),
            );
            window.paint_quad(fill(track, c.surface_hover).corner_radii(track_h / 2.0));
            let filled = Bounds::from_corners(
                track.origin,
                point(thumb_x + thumb / 2.0, mid + track_h / 2.0),
            );
            window.paint_quad(fill(filled, c.accent).corner_radii(track_h / 2.0));
            let knob = Bounds::from_corners(
                point(thumb_x, mid - thumb / 2.0),
                point(thumb_x + thumb, mid + thumb / 2.0),
            );
            window.paint_quad(fill(knob, c.text).corner_radii(thumb / 2.0));
        },
        on_change,
    )
    .h(size::SLIDER_HEIGHT)
    .w_full()
}

struct Tooltip(SharedString);

impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let c = theme.colors;
        theme
            .text(div(), typography::BODY_MUTED)
            .px(space::S2)
            .py(space::S1)
            .rounded(radius::S)
            .border_1()
            .border_color(c.line_strong)
            .bg(c.surface_raised)
            .text_color(c.text)
            .child(self.0.clone())
    }
}

/// Builder for `.tooltip(..)`, required on icon-only controls.
pub fn tooltip(text: &'static str) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let text = SharedString::new_static(text);
    move |_, cx| cx.new(|_| Tooltip(text.clone())).into()
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

/// What a [`toast`] is about; picks the status color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Warning,
    Error,
}

/// A floating message entering from below. `id` must change for each new toast
/// so the entrance plays again. Dismissal timing belongs to the caller.
pub fn toast(
    theme: &Theme,
    id: impl Into<ElementId>,
    kind: ToastKind,
    text: impl Into<SharedString>,
) -> impl IntoElement {
    let c = theme.colors;
    let color = match kind {
        ToastKind::Info => tokens::status::info(),
        ToastKind::Warning => tokens::status::warning(),
        ToastKind::Error => tokens::status::danger(),
    };
    let body = theme
        .text(div(), typography::BODY)
        .flex()
        .items_center()
        .gap(space::S3)
        .max_w(size::TOAST_MAX_WIDTH)
        .px(space::S4)
        .py(space::S3)
        .rounded(radius::L)
        .border_1()
        .border_color(c.line_strong)
        .bg(c.surface_raised)
        .text_color(c.text)
        .shadow(tokens::floating_shadow())
        .child(
            div()
                .flex_none()
                .size(size::STATUS_DOT)
                .rounded(radius::FULL)
                .bg(theme.readable(color)),
        )
        .child(text.into());
    motion::pop_in(id, body)
}

/// Data of one [`track_row`]; borrowed so the list never clones strings it
/// does not draw.
pub struct TrackRowData<'a> {
    /// 1-based position shown in the index column.
    pub index: usize,
    pub title: &'a str,
    pub artist: &'a str,
    /// Already formatted (`3:45`).
    pub duration: &'a str,
    pub artwork: Option<Arc<Path>>,
    /// This is the current track: accent title and the equalizer.
    pub active: bool,
    /// The current track is playing (the equalizer moves), otherwise it is paused.
    pub playing: bool,
    /// Label of the "preview only" badge, when the track has one.
    pub preview_badge: Option<&'a str>,
    /// Shown only while the row is hovered (see [`row_action`]).
    pub actions: Vec<AnyElement>,
}

/// The "now playing" equalizer: three bars, moving only while `playing`.
fn equalizer(theme: &Theme, playing: bool) -> Div {
    let color = theme.colors.accent;
    let max = f32::from(size::EQUALIZER_HEIGHT);
    div()
        .flex()
        .items_end()
        .gap(size::EQUALIZER_GAP)
        .h(size::EQUALIZER_HEIGHT)
        .children(
            motion::EQUALIZER
                .into_iter()
                .enumerate()
                .map(|(i, period)| {
                    let bar = div()
                        .w(size::EQUALIZER_BAR)
                        .rounded(size::EQUALIZER_BAR_RADIUS)
                        .bg(color)
                        .h(px(max * size::EQUALIZER_FROZEN[i]));
                    if !playing {
                        return bar.into_any_element();
                    }
                    bar.with_animation(
                        ("equalizer-bar", i),
                        Animation::new(period).repeat(),
                        move |bar, t| bar.h(px(max * equalizer_level(t, i))),
                    )
                    .into_any_element()
                }),
        )
}

/// Height of equalizer bar `bar` (as a fraction of the maximum) at loop time `t`.
fn equalizer_level(t: f32, bar: usize) -> f32 {
    let wave = 0.5 + 0.5 * (t * std::f32::consts::TAU + bar as f32).sin();
    0.25 + 0.75 * wave
}

/// Group name rows share, so their hover actions react to their own row.
const ROW_GROUP: &str = "track-row";

/// One result row: index (or equalizer), cover, title and artist, duration.
/// Fixed height ([`size::ROW_HEIGHT`]) so it fits `uniform_list`.
pub fn track_row(theme: &Theme, id: impl Into<ElementId>, row: TrackRowData) -> Stateful<Div> {
    let c = theme.colors;
    let cover = div()
        .flex_none()
        .size(size::ROW_COVER)
        .rounded(radius::M)
        .overflow_hidden()
        .bg(c.accent_soft)
        .when_some(row.artwork, |cover, path| {
            cover.child(img(path).size_full().object_fit(ObjectFit::Cover))
        });
    let index = div()
        .flex_none()
        .w(size::ROW_INDEX)
        .flex()
        .justify_center()
        .child(if row.active {
            equalizer(theme, row.playing).into_any_element()
        } else {
            theme
                .text(div(), typography::MONO)
                .text_color(c.text_subtle)
                .child(row.index.to_string())
                .into_any_element()
        });
    let title_color = if row.active { c.accent } else { c.text };

    theme
        .text(div(), typography::BODY)
        .id(id)
        .group(ROW_GROUP)
        .relative()
        .flex()
        .items_center()
        .gap(space::S3)
        .w_full()
        .h(size::ROW_HEIGHT)
        .px(space::S3)
        .rounded(radius::M)
        .border_1()
        .border_color(gpui::transparent_black())
        .tab_index(0)
        .focus_visible(move |s| s.border_color(c.accent))
        .cursor_pointer()
        .hover(move |s| s.bg(c.surface_raised))
        .child(index)
        .child(cover)
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .child(
                    div()
                        .truncate()
                        .text_color(title_color)
                        .child(row.title.to_owned()),
                )
                .child(
                    theme
                        .text(div(), typography::BODY_MUTED)
                        .truncate()
                        .text_color(c.text_muted)
                        .child(row.artist.to_owned()),
                ),
        )
        .when_some(row.preview_badge, |el, label| {
            el.child(badge(theme, label.to_owned(), tokens::status::warning()))
        })
        .child(
            theme
                .text(div(), typography::MONO)
                .flex_none()
                .text_color(c.text_subtle)
                .child(row.duration.to_owned()),
        )
        .when(!row.actions.is_empty(), |el| {
            // Laid out over the row's right edge, not in the flow, so the title
            // keeps its width. (Toggling `display` on hover breaks GPUI's
            // prepaint/paint pairing, so visibility is used instead.)
            el.child(
                div()
                    .absolute()
                    .right(space::S2)
                    .flex()
                    .items_center()
                    .gap(space::S1)
                    .p(space::S1)
                    .rounded(radius::S)
                    .bg(c.surface_raised)
                    .invisible()
                    .group_hover(ROW_GROUP, |s| s.visible())
                    .children(row.actions),
            )
        })
}

/// A track row in loading state: the same shape with a soft shimmer. The
/// shimmer is phase-locked, so every skeleton breathes together.
pub fn skeleton_row(theme: &Theme, id: impl Into<ElementId>) -> impl IntoElement {
    let c = theme.colors;
    let block = move |width: Pixels, height: Pixels| {
        div()
            .w(width)
            .h(height)
            .rounded(radius::S)
            .bg(c.surface_hover)
    };
    div()
        .flex()
        .items_center()
        .gap(space::S3)
        .w_full()
        .h(size::ROW_HEIGHT)
        .px(space::S3)
        .child(div().flex_none().w(size::ROW_INDEX))
        .child(
            div()
                .flex_none()
                .size(size::ROW_COVER)
                .rounded(radius::M)
                .bg(c.surface_hover),
        )
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .gap(space::S2)
                .child(block(
                    size::SKELETON_TITLE_WIDTH,
                    size::SKELETON_TITLE_HEIGHT,
                ))
                .child(block(
                    size::SKELETON_ARTIST_WIDTH,
                    size::SKELETON_ARTIST_HEIGHT,
                )),
        )
        .with_animation(
            id,
            Animation::new(motion::SHIMMER).repeat_synced(),
            |row, t| row.opacity(shimmer_opacity(t)),
        )
}

/// Opacity of the skeleton at loop time `t`: a slow breath between 0.45 and 1.
fn shimmer_opacity(t: f32) -> f32 {
    0.725 + 0.275 * (t * std::f32::consts::TAU).cos()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fraction_is_relative_to_the_strip() {
        assert_eq!(fraction_in(150.0, 100.0, 200.0), 0.25);
        assert_eq!(fraction_in(100.0, 100.0, 200.0), 0.0);
        assert_eq!(fraction_in(300.0, 100.0, 200.0), 1.0);
    }

    #[test]
    fn fraction_clamps_outside_the_strip() {
        assert_eq!(fraction_in(50.0, 100.0, 200.0), 0.0);
        assert_eq!(fraction_in(900.0, 100.0, 200.0), 1.0);
    }

    #[test]
    fn without_hover_bars_are_played_or_rest() {
        assert_eq!(bar_kind(0.2, 0.5, None), BarKind::Played);
        assert_eq!(bar_kind(0.8, 0.5, None), BarKind::Rest);
    }

    #[test]
    fn hovering_ahead_previews_the_bars_a_click_would_play() {
        assert_eq!(bar_kind(0.4, 0.5, Some(0.8)), BarKind::Played);
        assert_eq!(bar_kind(0.6, 0.5, Some(0.8)), BarKind::Preview);
        assert_eq!(bar_kind(0.9, 0.5, Some(0.8)), BarKind::Rest);
    }

    #[test]
    fn hovering_behind_previews_the_bars_a_click_would_give_up() {
        assert_eq!(bar_kind(0.2, 0.5, Some(0.3)), BarKind::Played);
        assert_eq!(bar_kind(0.4, 0.5, Some(0.3)), BarKind::Preview);
        assert_eq!(bar_kind(0.6, 0.5, Some(0.3)), BarKind::Rest);
    }

    #[test]
    fn fraction_of_an_unmeasured_strip_is_zero() {
        assert_eq!(fraction_in(10.0, 0.0, 0.0), 0.0);
    }

    #[test]
    fn animated_levels_stay_in_range() {
        for i in 0..=100 {
            let t = i as f32 / 100.0;
            for bar in 0..3 {
                assert!((0.25 - 1e-4..=1.0 + 1e-4).contains(&equalizer_level(t, bar)));
            }
            assert!((0.45 - 1e-4..=1.0 + 1e-4).contains(&shimmer_opacity(t)));
        }
    }
}
