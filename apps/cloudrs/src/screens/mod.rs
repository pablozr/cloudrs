//! The screens of the router (ADR 0008), drawn from the app-local view
//! models. Each is an `impl Shell` block in its own file; the one list
//! component they share is in `list`.

mod account;
mod history;
mod home;
mod jam;
mod library;
mod list;
mod playlist;
mod search;
mod track;
mod user;

use cloudrs_ui::browse::skeleton_header;
use cloudrs_ui::components::Icon;
use cloudrs_ui::tokens::space;
use cloudrs_ui::{Theme, motion};
use gpui::prelude::*;
use gpui::{AnyElement, Context, div};

pub use track::{TrackWave, WaveAction};

use crate::i18n;
use crate::nav::Route;
use crate::shell::{Shell, status_view};

impl Shell {
    /// The screen of the current route. A new `nav_seq` replays the entrance.
    pub(crate) fn screen(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let page = match self.router.current().clone() {
            Route::Home => self.home_screen(theme, cx),
            Route::Search => self.search_screen(theme, cx),
            Route::Track(id) => self.track_screen(id, theme, cx),
            Route::User(id) => self.user_screen(id, theme, cx),
            Route::Playlist(id) => self.playlist_screen(id, theme, cx),
            Route::History => self.history_screen(theme, cx),
            Route::Account => self.account_screen(theme, cx),
            Route::Jam => self.jam_screen(theme, cx),
            Route::Library => self.library_screen(theme, cx),
            route @ (Route::Feed | Route::Likes(_) | Route::Following(_)) => {
                let list = route.account_list().expect("an account list route");
                self.account_list_screen(list, account::account_list_title(list), theme, cx)
            }
            Route::Resolving(_) => div()
                .size_full()
                .flex()
                .flex_col()
                .child(div().pt(space::S6).child(skeleton_header(
                    theme,
                    "resolving-skeleton",
                    false,
                )))
                .child(div().flex_1().child(status_view(
                    theme,
                    Icon::Search,
                    "resolving",
                    i18n::page::resolving_title(),
                    i18n::page::resolving_hint(),
                    None,
                )))
                .into_any_element(),
        };
        motion::page_in(("page", self.nav_seq), div().size_full().child(page)).into_any_element()
    }
}
