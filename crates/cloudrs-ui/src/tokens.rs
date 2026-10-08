//! Design tokens: the single source of raw values, in both themes.
#![allow(clippy::unreadable_literal)]

use gpui::{BoxShadow, FontWeight, Hsla, Pixels, Rgba, point, px, rgb};

/// `0xRRGGBB` at `alpha`. Keeps hex and alpha apart so a six-digit color is
/// never read as eight digits.
fn tone(hex: u32, alpha: f32) -> Hsla {
    let mut color: Rgba = rgb(hex);
    color.a = alpha;
    color.into()
}

/// Colors that change between the dark and the light theme.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// Sidebar and player bar.
    pub canvas_deep: Hsla,
    pub canvas: Hsla,
    pub surface: Hsla,
    pub surface_raised: Hsla,
    pub surface_hover: Hsla,
    pub line: Hsla,
    pub line_strong: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub text_subtle: Hsla,
    /// Primary action, play, progress, selection, focus, liked.
    pub accent: Hsla,
    pub accent_hover: Hsla,
    pub accent_soft: Hsla,
    pub accent_glow: Hsla,
    /// Waveform bars the click would play (or un-play), while hovering.
    pub accent_preview: Hsla,
    /// Text on an accent fill.
    pub on_accent: Hsla,
}

pub fn dark() -> Palette {
    Palette {
        canvas_deep: tone(0x0D0C0B, 1.0),
        canvas: tone(0x141210, 1.0),
        surface: tone(0x1B1916, 1.0),
        surface_raised: tone(0x24211D, 1.0),
        surface_hover: tone(0x2F2B26, 1.0),
        line: tone(0xFFECDC, 0.08),
        line_strong: tone(0xFFECDC, 0.14),
        text: tone(0xF5EFE8, 1.0),
        text_muted: tone(0xB3AAA0, 1.0),
        text_subtle: tone(0x7D756C, 1.0),
        accent: tone(0xFF5500, 1.0),
        accent_hover: tone(0xFF7A1A, 1.0),
        accent_soft: tone(0xFF5500, 0.14),
        accent_glow: tone(0xFF5500, 0.35),
        accent_preview: tone(0xFF5500, 0.45),
        on_accent: tone(0xFFFFFF, 1.0),
    }
}

pub fn light() -> Palette {
    Palette {
        canvas_deep: tone(0xF2EDE6, 1.0),
        canvas: tone(0xFAF7F3, 1.0),
        surface: tone(0xFFFFFF, 1.0),
        surface_raised: tone(0xF2EDE6, 1.0),
        surface_hover: tone(0xEAE3DA, 1.0),
        line: tone(0x28190A, 0.09),
        line_strong: tone(0x28190A, 0.16),
        text: tone(0x1A1714, 1.0),
        text_muted: tone(0x5E564D, 1.0),
        text_subtle: tone(0x8F867C, 1.0),
        accent: tone(0xE84D00, 1.0),
        accent_hover: tone(0xFF5500, 1.0),
        accent_soft: tone(0xFF5500, 0.12),
        accent_glow: tone(0xFF5500, 0.28),
        accent_preview: tone(0xE84D00, 0.40),
        on_accent: tone(0xFFFFFF, 1.0),
    }
}

/// The accent gradient stops (`#FF3D00 → #FF5500 → #FF9A1F`, 135°).
pub fn accent_gradient() -> [Hsla; 3] {
    [
        tone(0xFF3D00, 1.0),
        tone(0xFF5500, 1.0),
        tone(0xFF9A1F, 1.0),
    ]
}

/// Status colors, the same in both themes. Never a replacement for the accent.
pub mod status {
    use super::*;
    pub fn success() -> Hsla {
        tone(0x3DD68C, 1.0)
    }
    pub fn warning() -> Hsla {
        tone(0xFFC145, 1.0)
    }
    pub fn danger() -> Hsla {
        tone(0xFF4D5E, 1.0)
    }
    pub fn info() -> Hsla {
        tone(0x5AA9FF, 1.0)
    }
}

/// Shadow of floating things (toasts, menus). Dark mode otherwise gets depth
/// from lighter surfaces.
pub fn floating_shadow() -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: tone(0x000000, 0.35),
        offset: point(px(0.0), px(8.0)),
        blur_radius: px(24.0),
        spread_radius: px(0.0),
        inset: false,
    }]
}

/// The keyboard focus ring: a solid outline that does not shift the layout.
pub fn focus_ring(color: Hsla) -> Vec<BoxShadow> {
    vec![BoxShadow {
        color,
        offset: point(px(0.0), px(0.0)),
        blur_radius: px(0.0),
        spread_radius: px(2.0),
        inset: false,
    }]
}

/// The 4 px spacing scale.
pub mod space {
    use super::*;
    pub const S1: Pixels = px(4.0);
    pub const S2: Pixels = px(8.0);
    pub const S3: Pixels = px(12.0);
    pub const S4: Pixels = px(16.0);
    pub const S5: Pixels = px(20.0);
    pub const S6: Pixels = px(24.0);
    pub const S7: Pixels = px(32.0);
    pub const S8: Pixels = px(48.0);
}

/// Corner radii.
pub mod radius {
    use super::*;
    /// Badges, kbd.
    pub const S: Pixels = px(6.0);
    /// Buttons, rows, covers.
    pub const M: Pixels = px(10.0);
    /// Cards, toasts.
    pub const L: Pixels = px(14.0);
    /// Panels, dialogs.
    pub const XL: Pixels = px(20.0);
    /// Pills, the play button, search.
    pub const FULL: Pixels = px(9999.0);
}

/// Which embedded face a text style uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontRole {
    Display,
    Interface,
    Mono,
}

/// One step of the type scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TypeToken {
    pub size: Pixels,
    pub line_height: Pixels,
    pub weight: FontWeight,
    pub role: FontRole,
}

const fn type_token(size: f32, line: f32, weight: f32, role: FontRole) -> TypeToken {
    TypeToken {
        size: px(size),
        line_height: px(line),
        weight: FontWeight(weight),
        role,
    }
}

/// The type scale.
pub mod typography {
    use super::*;
    pub const DISPLAY_XL: TypeToken = type_token(40.0, 44.0, 800.0, FontRole::Display);
    pub const DISPLAY_L: TypeToken = type_token(28.0, 34.0, 700.0, FontRole::Display);
    pub const TITLE: TypeToken = type_token(18.0, 24.0, 700.0, FontRole::Display);
    pub const BODY: TypeToken = type_token(14.0, 20.0, 500.0, FontRole::Interface);
    pub const BODY_MUTED: TypeToken = type_token(13.0, 18.0, 400.0, FontRole::Interface);
    pub const LABEL: TypeToken = type_token(11.0, 14.0, 600.0, FontRole::Interface);
    pub const MONO: TypeToken = type_token(12.0, 16.0, 400.0, FontRole::Mono);
}

/// The soft gradient at the top of the content area, tinted by the artwork's
/// colour. `Theme::tint` blends the colour per theme.
pub mod tint {
    use super::*;
    /// How far the gradient reaches down before it is gone.
    pub const HEIGHT: Pixels = px(360.0);
    /// Opacity at the top.
    pub const ALPHA_DARK: f32 = 0.26;
    pub const ALPHA_LIGHT: f32 = 0.16;
    /// Lightness limits, so a bright cover does not glare on dark and a dark
    /// one does not turn the light theme grey.
    pub const MAX_LIGHTNESS_DARK: f32 = 0.5;
    pub const MIN_LIGHTNESS_DARK: f32 = 0.22;
    pub const MIN_LIGHTNESS_LIGHT: f32 = 0.5;
    pub const MAX_LIGHTNESS_LIGHT: f32 = 0.78;
    pub const MAX_SATURATION: f32 = 0.85;
}

/// Opacity of a control that has nothing to act on yet.
pub const DISABLED_OPACITY: f32 = 0.4;

/// Bars of the placeholder waveform shown before a track has its own.
pub const PLACEHOLDER_BARS: usize = 96;
pub const PLACEHOLDER_LEVEL: f32 = 0.18;

/// Share of a waveform bar's slot taken by the gap, up to `size::WAVEFORM_GAP`.
pub const WAVEFORM_GAP_RATIO: f32 = 0.4;

/// Fixed dimensions of components.
pub mod size {
    use super::*;
    /// A track row; the list measures it once.
    pub const ROW_HEIGHT: Pixels = px(56.0);
    /// Cover in a track row.
    pub const ROW_COVER: Pixels = px(40.0);
    /// Index / equalizer column of a track row.
    pub const ROW_INDEX: Pixels = px(28.0);
    /// Cover in the player bar.
    pub const PLAYER_COVER: Pixels = px(52.0);
    pub const PLAYER_HEIGHT: Pixels = px(100.0);
    /// Width of the player bar's now-playing block.
    pub const PLAYER_INFO_WIDTH: Pixels = px(260.0);
    pub const PLAY_BUTTON: Pixels = px(44.0);
    pub const WAVEFORM_HEIGHT: Pixels = px(32.0);
    /// Widest gap between waveform bars, and the thinnest a bar gets on a
    /// narrow strip.
    pub const WAVEFORM_GAP: Pixels = px(2.0);
    pub const WAVEFORM_BAR_MIN: Pixels = px(0.5);
    /// Lucide icons (24 px grid, 2 px stroke): small (row actions), medium
    /// (buttons, sidebar, fields) and the muted one above an empty state.
    pub const ICON_S: Pixels = px(16.0);
    pub const ICON_M: Pixels = px(20.0);
    pub const ICON_STATUS: Pixels = px(40.0);
    /// The logo symbol next to the wordmark. Clear space is a quarter of it.
    pub const LOGO: Pixels = px(32.0);
    pub const LOGO_CLEAR_SPACE: Pixels = px(8.0);
    /// Round icon buttons (previous, next, shuffle, repeat, queue).
    pub const ICON_BUTTON: Pixels = px(34.0);
    /// The queue side panel.
    pub const QUEUE_PANEL_WIDTH: Pixels = px(360.0);
    /// The row that follows the pointer while it is dragged.
    pub const DRAG_PREVIEW_WIDTH: Pixels = px(320.0);
    pub const VOLUME_WIDTH: Pixels = px(96.0);
    /// Height of the slider's hit area; the visible track is thinner.
    pub const SLIDER_HEIGHT: Pixels = px(20.0);
    pub const SLIDER_TRACK: Pixels = px(4.0);
    pub const SLIDER_THUMB: Pixels = px(12.0);
    pub const EQUALIZER_BAR: Pixels = px(3.0);
    pub const EQUALIZER_HEIGHT: Pixels = px(14.0);
    pub const EQUALIZER_GAP: Pixels = px(2.0);
    pub const EQUALIZER_BAR_RADIUS: Pixels = px(1.0);
    /// Bar heights (fraction of the maximum) while the equalizer is frozen.
    pub const EQUALIZER_FROZEN: [f32; 3] = [0.55, 1.0, 0.75];
    pub const SKELETON_TITLE_HEIGHT: Pixels = px(12.0);
    pub const SKELETON_ARTIST_HEIGHT: Pixels = px(10.0);
    pub const STATUS_DOT: Pixels = px(8.0);
    pub const TOAST_MAX_WIDTH: Pixels = px(420.0);
    /// Longest the search field grows in the header.
    pub const SEARCH_MAX_WIDTH: Pixels = px(560.0);
    /// The key column of the shortcut list in Settings.
    pub const SHORTCUT_KEY_WIDTH: Pixels = px(160.0);
    /// The command palette, and the tallest its list grows before scrolling.
    pub const PALETTE_WIDTH: Pixels = px(560.0);
    pub const PALETTE_MAX_HEIGHT: Pixels = px(420.0);
    /// Skeleton text blocks.
    pub const SKELETON_TITLE_WIDTH: Pixels = px(220.0);
    pub const SKELETON_ARTIST_WIDTH: Pixels = px(140.0);
    pub const SKELETON_HEADER_TITLE_WIDTH: Pixels = px(320.0);
    /// The sidebar, and the accent rail on its active item.
    pub const SIDEBAR_WIDTH: Pixels = px(220.0);
    pub const SIDEBAR_ITEM_HEIGHT: Pixels = px(40.0);
    pub const SIDEBAR_RAIL: Pixels = px(3.0);
    /// Cover of a playlist in the sidebar.
    pub const SIDEBAR_COVER: Pixels = px(28.0);
    /// The app's own title bar (navigation, search, window controls).
    pub const TITLEBAR_HEIGHT: Pixels = px(56.0);
    /// A window control button (minimize, maximize, close), as on Windows 11.
    pub const WINDOW_BUTTON_WIDTH: Pixels = px(46.0);
    pub const WINDOW_BUTTON_HEIGHT: Pixels = px(32.0);
    /// A floating menu (add to playlist) and its rows.
    pub const MENU_WIDTH: Pixels = px(260.0);
    pub const MENU_MAX_HEIGHT: Pixels = px(320.0);
    pub const MENU_ITEM_HEIGHT: Pixels = px(36.0);
    /// A dialog (name a playlist, confirm a delete).
    pub const DIALOG_WIDTH: Pixels = px(520.0);
    /// The cover picked for a new playlist.
    pub const COVER_PICKER: Pixels = px(120.0);
    /// The strip of comment pins under the large waveform.
    pub const COMMENT_LANE: Pixels = px(16.0);
    /// A comment pin, and the one under the pointer or at the playhead.
    pub const COMMENT_PIN: Pixels = px(6.0);
    pub const COMMENT_PIN_ACTIVE: Pixels = px(10.0);
    /// A row of the comment list; the list measures it once.
    pub const COMMENT_ROW_HEIGHT: Pixels = px(64.0);
    /// The popover that opens over a comment pin.
    pub const COMMENT_POPOVER_WIDTH: Pixels = px(320.0);
    /// Avatars of the people in a Jam: in the title bar pill, and on its screen.
    pub const AVATAR_S: Pixels = px(24.0);
    pub const AVATAR_M: Pixels = px(40.0);
    /// How much stacked avatars overlap.
    pub const AVATAR_OVERLAP: Pixels = px(8.0);
    /// The crown over the host's avatar.
    pub const CROWN: Pixels = px(12.0);
    /// Artwork or avatar of a page header.
    pub const HEADER_ART: Pixels = px(160.0);
    /// The waveform of a track page.
    pub const WAVEFORM_LARGE_HEIGHT: Pixels = px(96.0);
    /// Longest the text of a track description grows on a page.
    pub const DESCRIPTION_MAX_WIDTH: Pixels = px(720.0);
    /// A card on the Home shelves, and its cover (the card's padding on each side).
    pub const CARD_WIDTH: Pixels = px(184.0);
    pub const CARD_COVER: Pixels = px(168.0);
    /// The play button that rises over a card's cover on hover.
    pub const CARD_PLAY: Pixels = px(40.0);
    /// The cover of the Home highlight.
    pub const HERO_ART: Pixels = px(176.0);
    /// A quick tile on Home: its height is the side of its cover.
    pub const QUICK_TILE: Pixels = px(56.0);
    pub const QUICK_PLAY: Pixels = px(32.0);
    /// Widest the Home content grows.
    pub const HOME_MAX_WIDTH: Pixels = px(1200.0);
}
