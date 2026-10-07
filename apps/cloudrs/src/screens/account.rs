//! The account screen (sign in, sign out) and the signed-in person's lists:
//! Feed, Likes, Library and Following (ADR 0010).

use cloudrs_ui::Theme;
use cloudrs_ui::components::{ButtonKind, Icon, button, icon};
use cloudrs_ui::tokens::{size, space, typography};
use gpui::prelude::*;
use gpui::{AnyElement, Context, div, px};

use crate::i18n::{self, account as t};
use crate::intent::UiIntent;
use crate::models::ListId;
use crate::shell::Shell;

impl Shell {
    pub(crate) fn account_screen(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let c = theme.colors;
        let body = match &self.models.account {
            Some(me) => div()
                .flex()
                .flex_col()
                .gap(space::S3)
                .child(
                    theme
                        .text(div(), typography::TITLE)
                        .flex()
                        .items_center()
                        .gap(space::S3)
                        .text_color(c.text)
                        .child(icon(Icon::Account, size::ICON_M, c.accent))
                        .child(me.username.clone()),
                )
                .child(muted(theme, t::signed_in_hint()))
                .child(
                    div().pt(space::S2).child(
                        button(theme, "sign-out", t::sign_out(), ButtonKind::Secondary)
                            .aria_label(t::sign_out())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.dispatch(UiIntent::SignOut, cx);
                            })),
                    ),
                ),
            None => {
                let signing_in = self.models.signing_in;
                let main = if signing_in {
                    muted(theme, t::waiting()).into_any_element()
                } else {
                    button(theme, "sign-in", t::sign_in(), ButtonKind::Primary)
                        .aria_label(t::sign_in())
                        .on_click(cx.listener(|this, _, _, cx| this.sign_in_with_window(cx)))
                        .into_any_element()
                };
                div()
                    .flex()
                    .flex_col()
                    .gap(space::S3)
                    .child(
                        theme
                            .text(div(), typography::TITLE)
                            .text_color(c.text)
                            .child(t::signed_out_title()),
                    )
                    .child(muted(theme, t::signed_out_hint()))
                    .child(div().pt(space::S2).child(main))
                    .child(div().mt(space::S6).h(px(1.0)).bg(c.line))
                    .child(
                        theme
                            .text(div(), typography::BODY)
                            .pt(space::S4)
                            .text_color(c.text)
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(t::other_ways()),
                    )
                    .child(muted(theme, t::token_steps()))
                    .child(self.token_field.clone())
                    .child(
                        div().child(
                            button(
                                theme,
                                "sign-in-token",
                                t::token_sign_in(),
                                ButtonKind::Outline,
                            )
                            .aria_label(t::token_sign_in())
                            .when(!signing_in, |b| {
                                b.on_click(
                                    cx.listener(|this, _, _, cx| this.sign_in_with_token(cx)),
                                )
                            }),
                        ),
                    )
            }
        };
        div()
            .id("account")
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
            .child(
                theme
                    .text(div(), typography::BODY_MUTED)
                    .pt(space::S6)
                    .text_color(c.text_subtle)
                    .child(t::unofficial()),
            )
            .into_any_element()
    }

    /// Feed, Likes, Library or Following: a title over the list.
    pub(crate) fn account_list_screen(
        &mut self,
        list: ListId,
        title: &'static str,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let view = self.list_view(list, title, theme, cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .px(space::S5)
            .child(
                theme
                    .text(div(), typography::DISPLAY_L)
                    .pt(space::S2)
                    .pb(space::S3)
                    .text_color(theme.colors.text)
                    .child(title),
            )
            .child(div().flex_1().min_h(px(0.0)).child(view))
            .into_any_element()
    }
}

fn muted(theme: &Theme, text: &'static str) -> gpui::Div {
    theme
        .text(div(), typography::BODY_MUTED)
        .text_color(theme.colors.text_muted)
        .child(text)
}

/// The title of an account list's screen.
pub fn account_list_title(list: ListId) -> &'static str {
    match list {
        ListId::Feed => i18n::nav::feed(),
        ListId::Library => i18n::nav::library(),
        ListId::Followings(_) => i18n::nav::following(),
        _ => i18n::nav::likes(),
    }
}
