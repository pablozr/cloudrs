//! A vertical overlay scrollbar for anything that scrolls (ADR 0027). GPUI
//! ships no scrollbar element, so this draws one from a [`ScrollHandle`].
//!
//! It is always visible while the content overflows and draws nothing (and
//! takes no pointer) when it does not. The thumb drags; a click on the track
//! centres the thumb there. The wheel and the keyboard are left alone.

use std::cell::Cell;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    App, Bounds, DispatchPhase, Div, Element, ElementId, GlobalElementId, Hitbox, HitboxBehavior,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Role,
    ScrollHandle, SharedString, Stateful, Style, UniformListScrollHandle, Window, div, fill, point,
    px, relative,
};

use crate::Theme;
use crate::tokens::{radius, size, space};

/// The bar for `handle`, laid over the right edge of its parent. The parent
/// must be `.relative()`, the bar must come after the content in the tree (the
/// scroll geometry is only fresh once the content has been prepainted), and the
/// content must be tracked with `.track_scroll(handle)`. `label` names it for
/// assistive technology.
pub fn scrollbar(
    id: impl Into<ElementId>,
    handle: &ScrollHandle,
    label: impl Into<SharedString>,
) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::ScrollBar)
        .aria_label(label)
        .absolute()
        .top_0()
        .bottom_0()
        .right_0()
        .w(size::SCROLLBAR_HIT)
        .child(ScrollbarElement {
            handle: handle.clone(),
        })
}

/// The handle a [`scrollbar`] needs for a `uniform_list` tracked with `scroll`.
pub fn list_scroll_handle(scroll: &UniformListScrollHandle) -> ScrollHandle {
    scroll.0.borrow().base_handle.clone()
}

/// `(top, len)` of the thumb inside a track of `track` pixels, for a viewport
/// of `viewport` pixels over content that scrolls `max_offset` further, now
/// scrolled by `scrolled`. `None` when there is nothing to scroll.
fn thumb(
    track: f32,
    viewport: f32,
    max_offset: f32,
    scrolled: f32,
    min_len: f32,
) -> Option<(f32, f32)> {
    if max_offset <= 0.0 || track <= 0.0 {
        return None;
    }
    let len = (track * viewport / (viewport + max_offset))
        .max(min_len)
        .min(track);
    let top = (track - len) * (scrolled / max_offset).clamp(0.0, 1.0);
    Some((top, len))
}

/// How far the content is scrolled when the thumb's top is `top` pixels down
/// a track of `track` pixels. The inverse of [`thumb`].
fn scrolled_for(top: f32, track: f32, len: f32, max_offset: f32) -> f32 {
    if track <= len {
        return 0.0;
    }
    (top / (track - len)).clamp(0.0, 1.0) * max_offset
}

/// Where the thumb is grabbed, measured from its top; `None` when it is not.
type Grab = Rc<Cell<Option<f32>>>;

struct ScrollbarElement {
    handle: ScrollHandle,
}

/// What `prepaint` measured, in window coordinates, for `paint`.
struct Geometry {
    hitbox: Hitbox,
    grab: Grab,
    track_top: f32,
    track: f32,
    /// Top and length of the thumb, relative to the track.
    top: f32,
    len: f32,
    max_offset: f32,
}

impl IntoElement for ScrollbarElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for ScrollbarElement {
    type RequestLayoutState = ();
    type PrepaintState = Option<Geometry>;

    fn id(&self) -> Option<ElementId> {
        Some("thumb".into())
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
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        _: &mut App,
    ) -> Option<Geometry> {
        let grab: Grab = window.with_element_state(id?, |state: Option<Grab>, _| {
            let grab = state.unwrap_or_default();
            (grab.clone(), grab)
        });
        let inset = f32::from(space::S1);
        let track = f32::from(bounds.size.height) - 2.0 * inset;
        let (top, len) = thumb(
            track,
            f32::from(self.handle.bounds().size.height),
            f32::from(self.handle.max_offset().y),
            -f32::from(self.handle.offset().y),
            f32::from(size::SCROLLBAR_MIN_THUMB),
        )?;
        Some(Geometry {
            hitbox: window.insert_hitbox(bounds, HitboxBehavior::BlockMouseExceptScroll),
            grab,
            track_top: f32::from(bounds.top()) + inset,
            track,
            top,
            len,
            max_offset: f32::from(self.handle.max_offset().y),
        })
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        geometry: &mut Option<Geometry>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(g) = geometry.take() else {
            return;
        };
        let c = Theme::of(cx).colors;
        let hovered = g.hitbox.is_hovered(window);
        let active = hovered || g.grab.get().is_some();
        if active {
            window.paint_quad(fill(bounds, c.line));
        }
        let thumb_width = size::SCROLLBAR_THUMB;
        let thumb_bounds = Bounds::new(
            point(
                bounds.left() + (bounds.size.width - thumb_width) / 2.0,
                px(g.track_top + g.top),
            ),
            gpui::size(thumb_width, px(g.len)),
        );
        let color = if active {
            c.scroll_thumb_active
        } else {
            c.scroll_thumb
        };
        window.paint_quad(fill(thumb_bounds, color).corner_radii(radius::FULL));

        let view = window.current_view();
        let handle = self.handle.clone();
        let Geometry {
            hitbox,
            grab,
            track_top,
            track,
            top,
            len,
            max_offset,
        } = g;
        let scroll_to = move |y: f32, grab_at: f32| {
            let scrolled = scrolled_for(y - track_top - grab_at, track, len, max_offset);
            handle.set_offset(point(handle.offset().x, px(-scrolled)));
        };

        window.on_mouse_event({
            let (grab, hitbox, scroll_to) = (grab.clone(), hitbox.clone(), scroll_to.clone());
            move |event: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble
                    || event.button != MouseButton::Left
                    || !hitbox.is_hovered(window)
                {
                    return;
                }
                let y = f32::from(event.position.y);
                let thumb_top = track_top + top;
                let grab_at = if (thumb_top..=thumb_top + len).contains(&y) {
                    y - thumb_top
                } else {
                    scroll_to(y, len / 2.0);
                    len / 2.0
                };
                grab.set(Some(grab_at));
                window.capture_pointer(hitbox.id);
                cx.stop_propagation();
                cx.notify(view);
            }
        });
        window.on_mouse_event({
            let grab = grab.clone();
            move |event: &MouseMoveEvent, _, window, cx| match grab.get() {
                Some(grab_at) if event.dragging() => {
                    scroll_to(f32::from(event.position.y), grab_at);
                    cx.notify(view);
                }
                Some(_) => {
                    // The button was released where no mouse-up reached us.
                    grab.set(None);
                    cx.notify(view);
                }
                None if hitbox.is_hovered(window) != hovered => cx.notify(view),
                None => {}
            }
        });
        window.on_mouse_event(move |event: &MouseUpEvent, _, _, cx| {
            if event.button == MouseButton::Left && grab.take().is_some() {
                cx.notify(view);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: f32 = 32.0;

    #[test]
    fn nothing_to_scroll_has_no_thumb() {
        assert_eq!(thumb(200.0, 200.0, 0.0, 0.0, MIN), None);
        assert_eq!(thumb(0.0, 200.0, 100.0, 0.0, MIN), None);
    }

    #[test]
    fn thumb_is_as_long_as_the_visible_share() {
        // Half of the content is visible: half of the track.
        let (top, len) = thumb(200.0, 200.0, 200.0, 0.0, MIN).unwrap();
        assert_eq!((top, len), (0.0, 100.0));
    }

    #[test]
    fn thumb_never_gets_shorter_than_the_minimum() {
        let (_, len) = thumb(200.0, 200.0, 100_000.0, 0.0, MIN).unwrap();
        assert_eq!(len, MIN);
    }

    #[test]
    fn thumb_never_gets_longer_than_the_track() {
        let (_, len) = thumb(20.0, 200.0, 1.0, 0.0, MIN).unwrap();
        assert_eq!(len, 20.0);
    }

    #[test]
    fn thumb_runs_from_the_top_to_the_end_of_the_track() {
        let (top, len) = thumb(200.0, 200.0, 200.0, 0.0, MIN).unwrap();
        assert_eq!(top, 0.0);
        let (top, len_end) = thumb(200.0, 200.0, 200.0, 200.0, MIN).unwrap();
        assert_eq!(len_end, len);
        assert_eq!(top, 200.0 - len);
        // An overscroll past the end stays on the track.
        let (top, _) = thumb(200.0, 200.0, 200.0, 250.0, MIN).unwrap();
        assert_eq!(top, 200.0 - len);
    }

    #[test]
    fn dragging_clamps_to_the_content() {
        assert_eq!(scrolled_for(-30.0, 200.0, 100.0, 400.0), 0.0);
        assert_eq!(scrolled_for(500.0, 200.0, 100.0, 400.0), 400.0);
        assert_eq!(scrolled_for(50.0, 200.0, 100.0, 400.0), 200.0);
    }

    #[test]
    fn a_thumb_that_fills_the_track_does_not_scroll() {
        assert_eq!(scrolled_for(10.0, 100.0, 100.0, 400.0), 0.0);
    }

    #[test]
    fn thumb_and_drag_are_inverse() {
        for scrolled in [0.0, 37.0, 150.0, 400.0] {
            let (top, len) = thumb(300.0, 250.0, 400.0, scrolled, MIN).unwrap();
            let back = scrolled_for(top, 300.0, len, 400.0);
            assert!((back - scrolled).abs() < 1e-3, "{scrolled} -> {back}");
        }
    }
}
