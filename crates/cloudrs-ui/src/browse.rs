//! Primitives of the browsing screens (ADR 0008): sidebar, tabs, page header
//! and the rows of people and playlists. Plain functions over plain data, like
//! [`crate::components`].

use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    Animation, AnimationExt, AnyElement, App, ClickEvent, Div, ElementId, Hsla, ObjectFit, Pixels,
    Role, SharedString, Stateful, Window, div, img, linear_color_stop, linear_gradient, px,
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
/// (or circle, with `radius::FULL`) otherwise.
fn picture(theme: &Theme, side: Pixels, corner: Pixels, artwork: Option<Arc<Path>>) -> Div {
    div()
        .flex_none()
        .size(side)
        .overflow_hidden()
        .bg(theme.colors.accent_soft)
        .rounded(corner)
        .when_some(artwork, |pic, path| {
            // The image is not clipped by its parent's corners: it rounds its own.
            pic.child(
                img(path)
                    .size_full()
                    .rounded(corner)
                    .object_fit(ObjectFit::Cover),
            )
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
        .child(picture(
            theme,
            size::HEADER_ART,
            corner(data.round),
            data.artwork,
        ))
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
    let avatar = picture(theme, size::ROW_COVER, radius::FULL, row.avatar);
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
    let cover = picture(theme, size::ROW_COVER, radius::M, row.cover);
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
    on_play: Option<impl Fn(&ClickEvent, &mut Window, &mut App) + 'static>,
) -> Stateful<Div> {
    let c = theme.colors;
    let id: ElementId = id.into();
    let cover = picture(theme, size::CARD_COVER, corner(data.round), data.cover)
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
                .size(size::CARD_COVER)
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

/// The corner of a picture: round for people, `l` for covers.
fn corner(round: bool) -> Pixels {
    if round { radius::FULL } else { radius::L }
}

/// Data of a [`hero`], the highlight on top of Home.
pub struct HeroData<'a> {
    /// A short label above the title (`NOW PLAYING`, `TRENDING #1`).
    pub eyebrow: &'a str,
    pub title: &'a str,
    pub meta: &'a str,
    pub cover: Option<Arc<Path>>,
    /// The cover's dominant colour, washed across the card.
    pub tint: Option<Hsla>,
    /// Buttons under the meta line.
    pub actions: Vec<AnyElement>,
}

/// A large card: big cover, label, display title, meta and actions, over a
/// wash of the cover's colour. Callers add the `aria_label`.
pub fn hero(theme: &Theme, id: impl Into<ElementId>, data: HeroData) -> Stateful<Div> {
    let c = theme.colors;
    let wash = data.tint.map(|color| {
        let color = theme.tint(color);
        div()
            .absolute()
            .top(px(0.0))
            .left(px(0.0))
            .size_full()
            .bg(linear_gradient(
                90.0,
                linear_color_stop(color, 0.0),
                linear_color_stop(color.opacity(0.0), 1.0),
            ))
    });
    div()
        .id(id)
        .relative()
        .overflow_hidden()
        .flex()
        .items_center()
        .gap(space::S6)
        .p(space::S6)
        .rounded(radius::XL)
        .bg(c.surface)
        .border_1()
        .border_color(c.line)
        .children(wash)
        .child(picture(theme, size::HERO_ART, radius::L, data.cover).relative())
        .child(
            div()
                .relative()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .gap(space::S2)
                .child(
                    theme
                        .text(div(), typography::LABEL)
                        .text_color(c.accent)
                        .child(data.eyebrow.to_owned()),
                )
                .child(
                    theme
                        .text(div(), typography::DISPLAY_XL)
                        .text_color(c.text)
                        .line_clamp(2)
                        .child(data.title.to_owned()),
                )
                .child(
                    theme
                        .text(div(), typography::BODY_MUTED)
                        .truncate()
                        .text_color(c.text_muted)
                        .child(data.meta.to_owned()),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space::S2)
                        .pt(space::S3)
                        .children(data.actions),
                ),
        )
}

/// Group name quick tiles share, so the play button reacts to its own tile.
const TILE_GROUP: &str = "quick-tile";

/// A compact shortcut on Home: cover on the left, title, and a play button
/// that shows on hover. Laid out to fill its share of a row. Callers add
/// `on_click` and the `aria_label`.
pub fn quick_tile(
    theme: &Theme,
    id: impl Into<ElementId> + Clone,
    title: &str,
    cover: Option<Arc<Path>>,
    round: bool,
    on_play: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let c = theme.colors;
    let id: ElementId = id.into();
    let corner = if round { radius::FULL } else { radius::M };
    theme
        .text(div(), typography::BODY)
        .id(id.clone())
        .group(TILE_GROUP)
        .flex_1()
        .min_w(px(0.0))
        .h(size::QUICK_TILE)
        .flex()
        .items_center()
        .gap(space::S3)
        .pr(space::S3)
        .rounded(radius::M)
        .overflow_hidden()
        .bg(c.surface_raised)
        .border_1()
        .border_color(gpui::transparent_black())
        .tab_index(0)
        .focus_visible(move |s| s.border_color(c.accent))
        .cursor_pointer()
        .hover(move |s| s.bg(c.surface_hover))
        .child(picture(theme, size::QUICK_TILE, corner, cover))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .text_color(c.text)
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(title.to_owned()),
        )
        .child(
            div()
                .invisible()
                .group_hover(TILE_GROUP, |s| s.visible())
                .child(
                    play_button(theme, (id, "play"), false, size::QUICK_PLAY).on_click(
                        move |event, window, cx| {
                            cx.stop_propagation();
                            on_play(event, window, cx);
                        },
                    ),
                ),
        )
}

/// A playlist in the sidebar's "Your playlists": small cover and title, the
/// accent title when it is the screen showing. Callers add `on_click` and the
/// `aria_label`.
pub fn sidebar_collection(
    theme: &Theme,
    id: impl Into<ElementId>,
    title: &str,
    cover: Option<Arc<Path>>,
    active: bool,
) -> Stateful<Div> {
    let c = theme.colors;
    theme
        .text(div(), typography::BODY)
        .id(id)
        .flex()
        .items_center()
        .gap(space::S3)
        .h(size::SIDEBAR_ITEM_HEIGHT)
        .px(space::S3)
        .rounded(radius::M)
        .border_1()
        .border_color(gpui::transparent_black())
        .tab_index(0)
        .focus_visible(move |s| s.border_color(c.accent))
        .cursor_pointer()
        .hover(move |s| s.bg(c.surface_raised).text_color(c.text))
        .text_color(if active { c.accent } else { c.text_muted })
        .child(picture(theme, size::SIDEBAR_COVER, radius::S, cover))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .child(title.to_owned()),
        )
}

/// Someone's picture: their avatar, or the initial of their name on a soft
/// accent circle while there is none.
pub fn avatar(theme: &Theme, side: Pixels, name: &str, image: Option<Arc<Path>>) -> Div {
    let c = theme.colors;
    let initial: String = name
        .chars()
        .find(|ch| ch.is_alphanumeric())
        .map(|ch| ch.to_uppercase().collect())
        .unwrap_or_default();
    let has_image = image.is_some();
    picture(theme, side, radius::FULL, image)
        .flex()
        .items_center()
        .justify_center()
        .when(!has_image, |pic| {
            pic.child(
                theme
                    .text(div(), typography::LABEL)
                    .text_color(c.accent)
                    .child(initial),
            )
        })
}

/// Overlapping avatars, each ringed in the background colour so they read
/// apart; the first one marked with a crown when it is the host's.
pub fn avatar_stack(
    theme: &Theme,
    people: Vec<(String, Option<Arc<Path>>)>,
    first_is_host: bool,
) -> Div {
    let c = theme.colors;
    div()
        .flex()
        .items_center()
        .children(people.into_iter().enumerate().map(|(ix, (name, image))| {
            div()
                .relative()
                .when(ix > 0, |a| a.ml(-size::AVATAR_OVERLAP))
                .rounded(radius::FULL)
                .border_2()
                .border_color(c.canvas)
                .child(avatar(theme, size::AVATAR_S, &name, image))
                .when(ix == 0 && first_is_host, |a| {
                    a.child(div().absolute().top(-space::S2).left(space::S1).child(icon(
                        Icon::Crown,
                        size::CROWN,
                        tokens::status::warning(),
                    )))
                })
        }))
}
