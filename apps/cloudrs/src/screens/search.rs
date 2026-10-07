//! Search: the tabs above the results of the chosen kind.

use cloudrs_ui::Theme;
use cloudrs_ui::browse::tabs;
use cloudrs_ui::tokens::space;
use gpui::prelude::*;
use gpui::{AnyElement, Context, div, px};

use crate::i18n::search as t;
use crate::intent::UiIntent;
use crate::models::{ListId, SEARCH_TABS, tab_index};
use crate::shell::Shell;

impl Shell {
    pub(crate) fn search_screen(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let kind = self.models.search_kind;
        let labels = [
            t::tab_tracks(),
            t::tab_people(),
            t::tab_playlists(),
            t::tab_albums(),
        ];
        let tabs = tabs(
            theme,
            "search-tabs",
            &labels,
            tab_index(kind),
            cx.processor(|this, ix: usize, _, cx| {
                this.dispatch(UiIntent::SetSearchKind(SEARCH_TABS[ix]), cx);
            }),
        );
        let list = self.list_view(ListId::Search { kind }, t::results(), theme, cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .px(space::S5)
            .child(div().pt(space::S2).pb(space::S3).child(tabs))
            .child(div().flex_1().min_h(px(0.0)).child(list))
            .into_any_element()
    }
}
