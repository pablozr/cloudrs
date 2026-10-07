//! The Jam screen (ADR 0011): start one and share its link, see who is
//! listening, and end or leave it.

use cloudrs_ui::Theme;
use cloudrs_ui::browse::{avatar, avatar_stack};
use cloudrs_ui::components::{
    ButtonKind, Icon, ToastKind, badge, button, icon, pill, row_action, tooltip,
};
use cloudrs_ui::tokens::{self, radius, size, space, typography};
use gpui::prelude::*;
use gpui::{AnyElement, ClipboardItem, Context, Div, div, px};
use sc_core::{JamRole, JamState};

use crate::i18n::jam as t;
use crate::intent::UiIntent;
use crate::shell::Shell;

impl Shell {
    pub(crate) fn jam_screen(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let c = theme.colors;
        let body = match self.models.jam.clone() {
            None => self.jam_intro(theme, cx),
            Some(state) => self.jam_live(&state, theme, cx),
        };
        div()
            .id("jam")
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .px(space::S5)
            .pb(space::S6)
            .child(
                theme
                    .text(div(), typography::DISPLAY_L)
                    .pt(space::S2)
                    .pb(space::S4)
                    .text_color(c.text)
                    .child(t::title()),
            )
            .child(div().max_w(size::SEARCH_MAX_WIDTH).child(body))
            .into_any_element()
    }

    /// No Jam yet: what it is, in three steps, and the button to start one.
    fn jam_intro(&mut self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let c = theme.colors;
        let step = |n: &'static str, title: &'static str, hint: &'static str| {
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .gap(space::S1)
                .p(space::S4)
                .rounded(radius::L)
                .bg(c.surface_raised)
                .child(
                    theme
                        .text(div(), typography::DISPLAY_L)
                        .text_color(c.accent)
                        .child(n),
                )
                .child(
                    theme
                        .text(div(), typography::BODY)
                        .text_color(c.text)
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(title),
                )
                .child(muted(theme, hint))
        };
        div()
            .relative()
            .overflow_hidden()
            .flex()
            .flex_col()
            .gap(space::S4)
            .p(space::S6)
            .rounded(radius::XL)
            .bg(c.surface)
            .border_1()
            .border_color(c.line)
            .child(div().absolute().top(px(0.0)).left(px(0.0)).size_full().bg(
                gpui::linear_gradient(
                    135.0,
                    gpui::linear_color_stop(c.accent.opacity(0.16), 0.0),
                    gpui::linear_color_stop(c.accent.opacity(0.0), 0.6),
                ),
            ))
            .child(
                div()
                    .relative()
                    .flex()
                    .items_center()
                    .gap(space::S3)
                    .child(icon(Icon::Jam, size::ICON_STATUS, c.accent))
                    .child(
                        theme
                            .text(div(), typography::DISPLAY_L)
                            .text_color(c.text)
                            .child(t::intro_title()),
                    ),
            )
            .child(div().relative().child(muted(theme, t::intro_hint())))
            .child(
                div()
                    .relative()
                    .flex()
                    .gap(space::S3)
                    .child(step("1", t::step_start(), t::step_start_hint()))
                    .child(step("2", t::step_share(), t::step_share_hint()))
                    .child(step("3", t::step_listen(), t::step_listen_hint())),
            )
            .child(
                div().relative().flex().pt(space::S2).child(
                    button(theme, "jam-start", t::start(), ButtonKind::Primary)
                        .aria_label(t::start())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.dispatch(UiIntent::StartJam, cx);
                        })),
                ),
            )
    }

    fn jam_live(&mut self, state: &JamState, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let c = theme.colors;
        let host = matches!(state.role, JamRole::Host);
        let heading = match (&state.role, state.connecting) {
            (JamRole::Host, true) => t::going_online().to_owned(),
            (JamRole::Guest { .. }, true) => t::joining().to_owned(),
            (JamRole::Host, false) => t::hosting().to_owned(),
            (JamRole::Guest { host }, false) => t::in_jam(host),
        };
        let mut view = div().flex().flex_col().gap(space::S3).child(
            theme
                .text(div(), typography::TITLE)
                .flex()
                .items_center()
                .gap(space::S3)
                .text_color(c.text)
                .child(icon(Icon::Jam, size::ICON_M, c.accent))
                .child(heading),
        );
        if let Some(link) = state.link.clone() {
            view = view
                .child(muted(theme, t::share_hint()))
                .child(
                    theme
                        .text(div(), typography::MONO)
                        .px(space::S3)
                        .py(space::S2)
                        .rounded(radius::M)
                        .border_1()
                        .border_color(c.line)
                        .bg(c.surface)
                        .text_color(c.text_muted)
                        .overflow_hidden()
                        .child(link.clone()),
                )
                .child(
                    div().flex().child(
                        button(theme, "jam-copy", t::copy_link(), ButtonKind::Primary)
                            .aria_label(t::copy_link())
                            .child(icon(Icon::Copy, size::ICON_S, c.on_accent))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(link.clone()));
                                this.show_toast(ToastKind::Info, t::copied(), cx);
                            })),
                    ),
                );
        } else if !host {
            view = view.child(muted(theme, t::guest_hint()));
        }

        view = view.child(
            theme
                .text(div(), typography::BODY)
                .pt(space::S4)
                .text_color(c.text)
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(t::people()),
        );
        if state.people.is_empty() {
            view = view.child(muted(theme, t::nobody_yet()));
        }
        for (ix, person) in state.people.iter().enumerate() {
            let id = person.id;
            let label = t::remove_person(&person.name);
            let picture = person
                .user
                .and_then(|user| self.models.art.users.get(&user).cloned());
            let role = if person.host {
                t::role_host()
            } else {
                t::role_guest()
            };
            let row = div()
                .flex()
                .items_center()
                .gap(space::S3)
                .p(space::S2)
                .rounded(radius::L)
                .bg(c.surface)
                .border_1()
                .border_color(c.line)
                .child(avatar(theme, size::AVATAR_M, &person.name, picture))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex()
                        .flex_col()
                        .child(
                            theme
                                .text(div(), typography::BODY)
                                .flex()
                                .items_center()
                                .gap(space::S1)
                                .truncate()
                                .text_color(c.text)
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(person.name.clone())
                                .when(person.host, |name| {
                                    name.child(icon(
                                        Icon::Crown,
                                        size::ICON_S,
                                        tokens::status::warning(),
                                    ))
                                }),
                        )
                        .child(
                            theme
                                .text(div(), typography::BODY_MUTED)
                                .text_color(c.text_muted)
                                .child(role),
                        ),
                )
                .when(person.cannot_play, |row| {
                    row.child(badge(theme, t::cannot_play(), tokens::status::warning()))
                })
                .when(host && !person.host, |row| {
                    row.child(
                        row_action(
                            theme,
                            ("jam-remove", ix),
                            Icon::Remove,
                            t::remove(),
                            cx.listener(move |this, _, _, cx| {
                                this.dispatch(UiIntent::RemoveFromJam(id), cx);
                            }),
                        )
                        .aria_label(label),
                    )
                });
            view = view.child(row);
        }

        if host {
            let on = state.guests_control_playback;
            let choice = |ix: usize, label: &'static str, selected: bool| {
                pill(theme, ("jam-perms", ix), label, selected)
                    .tab_index(0)
                    .focus_visible(move |s| s.border_color(c.accent))
                    .aria_label(label)
                    .tooltip(tooltip(label))
            };
            view = view.child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(space::S2)
                    .pt(space::S4)
                    .child(choice(0, t::guests_add_only(), !on).on_click(cx.listener(
                        |this, _, _, cx| this.dispatch(UiIntent::SetJamGuestsControl(false), cx),
                    )))
                    .child(choice(1, t::guests_control(), on).on_click(cx.listener(
                        |this, _, _, cx| this.dispatch(UiIntent::SetJamGuestsControl(true), cx),
                    ))),
            );
        }

        let (label, id) = if host {
            (t::end(), "jam-end")
        } else {
            (t::leave(), "jam-leave")
        };
        view.child(
            div().flex().pt(space::S6).child(
                button(theme, id, label, ButtonKind::Secondary)
                    .aria_label(label)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.dispatch(UiIntent::LeaveJam, cx);
                    })),
            ),
        )
    }
}

fn muted(theme: &Theme, text: &'static str) -> Div {
    theme
        .text(div(), typography::BODY_MUTED)
        .text_color(theme.colors.text_muted)
        .child(text)
}

/// Avatars shown in the title bar pill.
const PILL_AVATARS: usize = 4;

impl Shell {
    /// While in a Jam: a pill in the title bar with everyone's avatars (the
    /// host first, crowned) and how many listen. A click opens the Jam.
    pub(crate) fn jam_pill(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.models.jam.as_ref()?;
        let c = theme.colors;
        let me = self.models.account.as_ref().map_or_else(
            || (t::you().to_owned(), None),
            |me| {
                (
                    me.username.clone(),
                    self.models.art.users.get(&me.id).cloned(),
                )
            },
        );
        let others = state.people.iter().map(|p| {
            let picture = p
                .user
                .and_then(|user| self.models.art.users.get(&user).cloned());
            (p.name.clone(), picture)
        });
        // The host leads the stack: this person when hosting, else the host.
        let mut people: Vec<_> = match state.role {
            JamRole::Host => std::iter::once(me).chain(others).collect(),
            JamRole::Guest { .. } => {
                let mut all: Vec<_> = others.collect();
                let at = usize::from(!all.is_empty());
                all.insert(at, me);
                all
            }
        };
        let count = people.len();
        people.truncate(PILL_AVATARS);
        let label = if state.connecting {
            t::connecting().to_owned()
        } else {
            t::listening(count)
        };
        Some(
            div()
                .id("jam-pill")
                .occlude()
                .flex()
                .flex_none()
                .items_center()
                .gap(space::S2)
                .h(size::ICON_BUTTON)
                .pl(space::S2)
                .pr(space::S3)
                .rounded(radius::FULL)
                .bg(c.accent_soft)
                .border_1()
                .border_color(c.accent.opacity(0.35))
                .cursor_pointer()
                .hover(move |s| s.border_color(c.accent))
                .tab_index(0)
                .focus_visible(move |s| s.border_color(c.accent))
                .aria_label(t::open_jam())
                .tooltip(tooltip(t::open_jam()))
                .child(icon(Icon::Jam, size::ICON_S, c.accent))
                .child(avatar_stack(theme, people, true))
                .child(
                    theme
                        .text(div(), typography::BODY_MUTED)
                        .text_color(c.text)
                        .child(label),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.navigate(crate::nav::Route::Jam, cx);
                }))
                .into_any_element(),
        )
    }
}
