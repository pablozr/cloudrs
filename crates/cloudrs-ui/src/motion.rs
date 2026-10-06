//! Motion: curves, the duration catalog and entrance helpers.
//!
//! A duration always comes from this module, never a literal in a screen.
//! GPUI skips one-shot animations when the OS asks to reduce motion.

use std::time::Duration;

use gpui::prelude::*;
use gpui::{Animation, AnimationElement, AnimationExt, ElementId, px};

/// A CSS `cubic-bezier(x1, y1, x2, y2)` timing function.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CubicBezier {
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
}

impl CubicBezier {
    pub const fn new(x1: f32, y1: f32, x2: f32, y2: f32) -> Self {
        Self { x1, y1, x2, y2 }
    }

    /// One coordinate of the curve at parameter `t` (endpoints 0 and 1).
    fn axis(t: f32, p1: f32, p2: f32) -> f32 {
        let u = 1.0 - t;
        3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t
    }

    fn axis_slope(t: f32, p1: f32, p2: f32) -> f32 {
        let u = 1.0 - t;
        3.0 * u * u * p1 + 6.0 * u * t * (p2 - p1) + 3.0 * t * t * (1.0 - p2)
    }

    /// Eased value for linear progress `x` in 0..=1.
    pub fn eval(&self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        if x == 0.0 || x == 1.0 {
            return x;
        }
        // Newton's method on x(t) = x, falling back to bisection.
        let mut t = x;
        for _ in 0..8 {
            let error = Self::axis(t, self.x1, self.x2) - x;
            if error.abs() < 1e-5 {
                return Self::axis(t, self.y1, self.y2);
            }
            let slope = Self::axis_slope(t, self.x1, self.x2);
            if slope.abs() < 1e-6 {
                break;
            }
            t = (t - error / slope).clamp(0.0, 1.0);
        }
        let (mut low, mut high) = (0.0_f32, 1.0_f32);
        for _ in 0..30 {
            t = (low + high) / 2.0;
            if Self::axis(t, self.x1, self.x2) < x {
                low = t;
            } else {
                high = t;
            }
        }
        Self::axis(t, self.y1, self.y2)
    }
}

/// `ease.std`: hover, press, color changes.
pub const EASE_STD: CubicBezier = CubicBezier::new(0.2, 0.0, 0.0, 1.0);
/// `ease.out`: things arriving.
pub const EASE_OUT: CubicBezier = CubicBezier::new(0.16, 1.0, 0.3, 1.0);
/// `ease.spring`: a small overshoot (like, play, a dropped queue item).
pub const EASE_SPRING: CubicBezier = CubicBezier::new(0.34, 1.56, 0.64, 1.0);

/// A duration along a curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionSpec {
    pub duration_ms: u64,
    pub curve: CubicBezier,
}

impl MotionSpec {
    pub const fn new(duration_ms: u64, curve: CubicBezier) -> Self {
        Self { duration_ms, curve }
    }

    pub fn duration(&self) -> Duration {
        Duration::from_millis(self.duration_ms)
    }

    /// A one-shot GPUI animation for this spec.
    pub fn animation(&self) -> Animation {
        let curve = self.curve;
        Animation::new(self.duration()).with_easing(move |t| curve.eval(t))
    }
}

/// Hover, press, color changes.
pub const FAST: MotionSpec = MotionSpec::new(120, EASE_STD);
/// Icons, buttons, show and hide.
pub const BASE: MotionSpec = MotionSpec::new(200, EASE_OUT);
/// Toasts, queue reorder, popovers.
pub const SLOW: MotionSpec = MotionSpec::new(320, EASE_SPRING);
/// A screen replacing another.
pub const PAGE: MotionSpec = MotionSpec::new(420, EASE_OUT);

/// Content arriving: fades in while settling 6 px up into place. Keyed by
/// `id`, it plays once when the element first appears.
pub fn content_in<E>(id: impl Into<ElementId>, element: E) -> AnimationElement<E>
where
    E: Styled + IntoElement + 'static,
{
    element.with_animation(id, PAGE.animation(), |element, t| {
        element.relative().opacity(t).top(px(6.0 * (1.0 - t)))
    })
}

/// A toast or popover arriving from below with a small overshoot.
pub fn pop_in<E>(id: impl Into<ElementId>, element: E) -> AnimationElement<E>
where
    E: Styled + IntoElement + 'static,
{
    element.with_animation(id, SLOW.animation(), |element, t| {
        element
            .relative()
            .opacity(t.min(1.0))
            .top(px(12.0 * (1.0 - t)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_start_and_end_in_place() {
        for curve in [EASE_STD, EASE_OUT, EASE_SPRING] {
            assert_eq!(curve.eval(0.0), 0.0);
            assert_eq!(curve.eval(1.0), 1.0);
        }
    }

    #[test]
    fn linear_curve_is_identity() {
        let linear = CubicBezier::new(1.0 / 3.0, 1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0);
        for i in 0..=10 {
            let x = i as f32 / 10.0;
            assert!((linear.eval(x) - x).abs() < 1e-3);
        }
    }

    #[test]
    fn ease_out_is_ahead_of_linear() {
        assert!(EASE_OUT.eval(0.3) > 0.6);
    }

    #[test]
    fn spring_overshoots() {
        let peak = (1..100)
            .map(|i| EASE_SPRING.eval(i as f32 / 100.0))
            .fold(0.0, f32::max);
        assert!(peak > 1.0, "{peak}");
    }
}
