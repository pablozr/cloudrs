//! History: the tracks played, newest first.

use cloudrs_ui::Theme;
use cloudrs_ui::tokens::{space, typography};
use gpui::prelude::*;
use gpui::{AnyElement, Context, div, px};

use crate::i18n;
use crate::models::ListId;
use crate::shell::Shell;

impl Shell {
    pub(crate) fn history_screen(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let list = self.list_view(ListId::History, i18n::nav::history(), theme, cx);
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
                    .child(i18n::nav::history()),
            )
            .child(div().flex_1().min_h(px(0.0)).child(list))
            .into_any_element()
    }
}
