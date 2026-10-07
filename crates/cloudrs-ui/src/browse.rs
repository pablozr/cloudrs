//! Primitives of the browsing screens (ADR 0008): sidebar, tabs, page header
//! and the rows of people and playlists. Plain functions over plain data, like
//! [`crate::components`].

use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    Animation, AnimationExt, AnyElement, App, Div, ElementId, Hsla, ObjectFit, Pixels, Role,
    SharedString, Stateful, Window, div, img, px,
};

use crate::components::{Icon, badge, icon, play_button, shimmer_opacity};
use crate::tokens::{self, radius, size, space, typography};
use crate::{Theme, motion};

/// A sidebar entry. The active one shows the accent rail and a raised
/// background. Callers add `on_click` and the `aria_label`.
pub fn sidebar_item(
    theme: &Theme,
    id: impl Into<ElementId>,
    glyph: Icon,
    label: impl Into<SharedString>,
    active: bool,
) -> Stateful<Div> {
    let c = theme.colors;
    let tint = if active { c.accent } else { c.text_muted };
    theme
        .text(div(), typography::BODY)
        .id(id)
        .relative()
        .flex()
        .items_center()
        .gap(space::S3)
        .h(size::SIDEBAR_ITEM_HEIGHT)
        .px(space::S4)
        .rounded(radius::M)
        .border_1()
        .border_color(gpui::transparent_black())
        .tab_index(0)
        .focus_visible(move |s| s.border_color(c.accent))
        .cursor_pointer()
        .when(active, |item| item.bg(c.surface_raised).text_color(c.text))
        .when(!active, |item| {
            item.text_color(c.text_muted)
                .hover(move |s| s.bg(c.surface_raised).text_color(c.text))
        })
        .child(
            div()
                .absolute()
                .left(px(0.0))
                .top(space::S2)
                .bottom(space::S2)
                .w(size::SIDEBAR_RAIL)
                .rounded(radius::FULL)
                .when(active, |rail| rail.bg(c.accent)),
        )
        .child(icon(glyph, size::ICON_M, tint))
        .child(label.into())
}

/// A row of tabs, drawn as filter pills (the selected one is inverted).
/// `on_select` gets the index of the clicked tab.
pub fn tabs(
    theme: &Theme,
    id: &'static str,
    labels: &[&'static str],
    selected: usize,
    on_select: impl Fn(usize, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let on_select = Rc::new(on_select);
    let accent = theme.colors.accent;
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(space::S2)
        .role(Role::TabList)
        .children(labels.iter().enumerate().map(|(i, label)| {
            let on_select = on_select.clone();
            crate::components::pill(theme, (id, i), *label, i == selected)
                .role(Role::Tab)
                .tab_index(0)
                .focus_visible(move |s| s.border_color(accent))
                .aria_label(*label)
                .on_click(move |_, window, cx| on_select(i, window, cx))
        }))
}

/// Data of a [`page_header`]; borrowed like a row's.
pub struct PageHeaderData<'a> {
    pub artwork: Option<Arc<Path>>,
    /// An avatar (round) instead of a cover.
    pub round: bool,
    pub title: &'a str,
    /// Already formatted (`12 tracks · 48:10`).
    pub meta: &'a str,
    /// Buttons under the meta line.
    pub actions: Vec<AnyElement>,
}

/// A picture that is the artwork when there is one and a soft accent square
/// (or circle) otherwise.
fn picture(theme: &Theme, side: Pixels, round: bool, artwork: Option<Arc<Path>>) -> Div {
    div()
        .flex_none()
        .size(side)
        .overflow_hidden()
        .bg(theme.colors.accent_soft)
        .rounded(if round { radius::FULL } else { radius::L })
        .when_some(artwork, |pic, path| {
            pic.child(img(path).size_full().object_fit(ObjectFit::Cover))
        })
}

/// The top of a track, profile or playlist page: large artwork or avatar,
/// display title, meta line and actions.
pub fn page_header(theme: &Theme, data: PageHeaderData) -> Div {
    let c = theme.colors;
    div()
        .flex()
        .items_center()
        .gap(space::S6)
        .px(space::S5)
        .pb(space::S5)
        .child(picture(theme, size::HEADER_ART, data.round, data.artwork))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .gap(space::S2)
                .child(
                    theme
                        .text(div(), tokens::typography::DISPLAY_XL)
                        .line_clamp(2)
                        .text_color(c.text)
                        .child(data.title.to_owned()),
                )
                .child(
                    theme
                        .text(div(), typography::BODY_MUTED)
                        .truncate()
                        .text_color(c.text_muted)
                        .child(data.meta.to_owned()),
                )
                .when(!data.actions.is_empty(), |col| {
                    col.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(space::S2)
                            .pt(space::S2)
                            .children(data.actions),
                    )
                }),
        )
}

/// A page header in loading state: the same shape with the soft shimmer.
pub fn skeleton_header(theme: &Theme, id: impl Into<ElementId>, round: bool) -> impl IntoElement {
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
        .gap(space::S6)
        .px(space::S5)
        .pb(space::S5)
        .child(
            div()
                .flex_none()
                .size(size::HEADER_ART)
                .bg(c.surface_hover)
                .rounded(if round { radius::FULL } else { radius::L }),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(space::S3)
                .child(block(size::SKELETON_HEADER_TITLE_WIDTH, px(32.0)))
                .child(block(
                    size::SKELETON_TITLE_WIDTH,
                    size::SKELETON_TITLE_HEIGHT,
                )),
        )
        .with_animation(
            id,
            Animation::new(motion::SHIMMER).repeat_synced(),
            |header, t| header.opacity(shimmer_opacity(t)),
        )
}

/// The shell of a list row of people or playlists: same height as a track row
/// so `uniform_list` can measure it once.
fn browse_row(
    theme: &Theme,
    id: impl Into<ElementId>,
    picture: Div,
    title: &str,
    subtitle: &str,
    badge_label: Option<(&str, Hsla)>,
) -> Stateful<Div> {
    let c = theme.colors;
    theme
        .text(div(), typography::BODY)
        .id(id)
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
        .child(div().flex_none().w(size::ROW_INDEX))
        .child(picture)
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .child(div().truncate().text_color(c.text).child(title.to_owned()))
                .child(
                    theme
                        .text(div(), typography::BODY_MUTED)
                        .truncate()
                        .text_color(c.text_muted)
                        .child(subtitle.to_owned()),
                ),
        )
        .when_some(badge_label, |row, (label, color)| {
            row.child(badge(theme, label.to_owned(), color))
        })
}

/// Data of a [`user_row`].
pub struct UserRowData<'a> {
    pub name: &'a str,
    /// Already formatted (`12K followers`).
    pub meta: &'a str,
    pub avatar: Option<Arc<Path>>,
}

/// A person in a list: round avatar, name, meta line.
pub fn user_row(theme: &Theme, id: impl Into<ElementId>, row: UserRowData) -> Stateful<Div> {
    let avatar = picture(theme, size::ROW_COVER, true, row.avatar);
    browse_row(theme, id, avatar, row.name, row.meta, None)
}

/// Data of a [`collection_row`] (a playlist or an album).
pub struct CollectionRowData<'a> {
    pub title: &'a str,
    /// The owner, and the track count (`Ana · 12 tracks`).
    pub meta: &'a str,
    pub cover: Option<Arc<Path>>,
    /// Label of the "Album" badge, set when the collection is an album.
    pub album_badge: Option<&'a str>,
}

/// A playlist or album in a list: square cover, title, meta line and, for an
/// album, the badge.
pub fn collection_row(
    theme: &Theme,
    id: impl Into<ElementId>,
    row: CollectionRowData,
) -> Stateful<Div> {
    let cover = picture(theme, size::ROW_COVER, false, row.cover).rounded(radius::M);
    let badge_label = row.album_badge.map(|label| (label, theme.colors.accent));
    browse_row(theme, id, cover, row.title, row.meta, badge_label)
}

/// Data of a [`card`]; borrowed like a row's.
pub struct CardData<'a> {
    pub title: &'a str,
    /// Already formatted (`42 tracks`, `Artist`).
    pub meta: &'a str,
    pub cover: Option<Arc<Path>>,
    /// An avatar (round) instead of a cover.
    pub round: bool,
}

/// Group name cards share, so the play button reacts to its own card.
const CARD_GROUP: &str = "card";

/// A card on a Home shelf: square cover, title and meta line. The play button
/// rises over the cover on hover and handles its own click, so the card's
/// click (open) does not fire. Callers add `on_click` and the `aria_label`.
pub fn card(
    theme: &Theme,
    id: impl Into<ElementId> + Clone,
    data: CardData,
    on_play: Option<impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static>,
) -> Stateful<Div> {
    let c = theme.colors;
    let id: ElementId = id.into();
    let cover = picture(theme, size::CARD_WIDTH, data.round, data.cover)
        .relative()
        .when_some(on_play, |cover, on_play| {
            cover.child(
                div()
                    .absolute()
                    .right(space::S2)
                    .bottom(space::S2)
                    .invisible()
                    .group_hover(CARD_GROUP, |s| s.visible())
                    .child(
                        play_button(theme, (id.clone(), "play"), false, size::CARD_PLAY).on_click(
                            move |event, window, cx| {
                                cx.stop_propagation();
                                on_play(event, window, cx);
                            },
                        ),
                    ),
            )
        });
    theme
        .text(div(), typography::BODY)
        .id(id.clone())
        .group(CARD_GROUP)
        .flex_none()
        .w(size::CARD_WIDTH)
        .flex()
        .flex_col()
        .gap(space::S2)
        .p(space::S2)
        .rounded(radius::L)
        .border_1()
        .border_color(gpui::transparent_black())
        .tab_index(0)
        .focus_visible(move |s| s.border_color(c.accent))
        .cursor_pointer()
        .hover(move |s| s.bg(c.surface_raised))
        .child(cover)
        .child(
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .truncate()
                        .text_color(c.text)
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(data.title.to_owned()),
                )
                .child(
                    theme
                        .text(div(), typography::BODY_MUTED)
                        .truncate()
                        .text_color(c.text_muted)
                        .child(data.meta.to_owned()),
                ),
        )
}

/// A card in loading state: the same shape with the soft shimmer.
pub fn skeleton_card(theme: &Theme, id: impl Into<ElementId>) -> impl IntoElement {
    let c = theme.colors;
    div()
        .flex_none()
        .w(size::CARD_WIDTH)
        .flex()
        .flex_col()
        .gap(space::S3)
        .p(space::S2)
        .child(
            div()
                .size(size::CARD_WIDTH)
                .rounded(radius::L)
                .bg(c.surface_hover),
        )
        .child(
            div()
                .w(size::SKELETON_ARTIST_WIDTH)
                .h(size::SKELETON_TITLE_HEIGHT)
                .rounded(radius::S)
                .bg(c.surface_hover),
        )
        .with_animation(
            id,
            Animation::new(motion::SHIMMER).repeat_synced(),
            |card, t| card.opacity(shimmer_opacity(t)),
        )
}
