//! The cloudrs design system for GPUI.
//!
//! Rules (see `docs/design/VISUAL-IDENTITY.md`):
//! - [`tokens`] is the only place with raw colors, sizes and durations.
//! - Screens reach tokens through [`Theme`], never by value.
//! - Motion goes through [`motion`]; GPUI honours the OS "reduce motion" setting.

pub mod components;
pub mod fonts;
pub mod motion;
mod search_edit;
pub mod search_field;
pub mod theme;
pub mod tokens;

pub use theme::{Theme, ThemeMode};
