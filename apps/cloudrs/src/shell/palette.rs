//! The command palette (ADR 0018): Ctrl P jumps to any screen or runs an
//! action. The surface is `cloudrs_ui::palette`; the items, the filter and
//! what a row does live here. Filtering happens when the field changes, never
//! in `render`.

use cloudrs_ui::Theme;
use cloudrs_ui::components::Icon;
use cloudrs_ui::palette::{self as surface, palette, palette_row};
use cloudrs_ui::search_field::SearchChanged;
use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Div, Focusable, KeyBinding, ScrollHandle, Window, actions};
use sc_core::{Command, ThemeChoice};

use super::{Shell, shortcuts};
use crate::i18n::{self, palette as t};
use crate::intent::UiIntent;
use crate::nav::Route;

actions!(
    palette,
    [
        TogglePalette,
        PaletteUp,
        PaletteDown,
        PaletteConfirm,
        PaletteClose
    ]
);

/// The palette's key bindings. They must be registered after
/// `search_field::bind_keys`: the field binds Escape to "clear", and at equal
/// depth the binding added last wins, so Escape closes the palette while its
/// field has focus.
pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("secondary-p", TogglePalette, None),
        KeyBinding::new("up", PaletteUp, Some(surface::CONTEXT)),
        KeyBinding::new("down", PaletteDown, Some(surface::CONTEXT)),
        KeyBinding::new("enter", PaletteConfirm, Some(surface::CONTEXT)),
        // The deeper context beats the field's own Escape.
        KeyBinding::new("escape", PaletteClose, Some("CommandPalette > SearchField")),
        KeyBinding::new("escape", PaletteClose, Some(surface::CONTEXT)),
    ]);
}

/// What the palette lists: every screen without parameters, then actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaletteCommand {
    Home,
    Search,
    History,
    Jam,
    Account,
    Settings,
    Feed,
    Likes,
    Library,
    Following,
    TogglePlay,
    Previous,
    Next,
    Like,
    Queue,
    ThemeSystem,
    ThemeDark,
    ThemeLight,
    CheckForUpdates,
    MiniPlayer,
}

impl PaletteCommand {
    const ALL: [Self; 20] = [
        Self::Home,
        Self::Search,
        Self::History,
        Self::Jam,
        Self::Account,
        Self::Settings,
        Self::Feed,
        Self::Likes,
        Self::Library,
        Self::Following,
        Self::TogglePlay,
        Self::Previous,
        Self::Next,
        Self::Like,
        Self::Queue,
        Self::ThemeSystem,
        Self::ThemeDark,
        Self::ThemeLight,
        Self::CheckForUpdates,
        Self::MiniPlayer,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Home => i18n::nav::home(),
            Self::Search => i18n::nav::search(),
            Self::History => i18n::nav::history(),
            Self::Jam => i18n::nav::jam(),
            Self::Account => i18n::account::title(),
            Self::Settings => i18n::settings::title(),
            Self::Feed => i18n::nav::feed(),
            Self::Likes => i18n::nav::likes(),
            Self::Library => i18n::nav::library(),
            Self::Following => i18n::nav::following(),
            Self::TogglePlay => t::play_pause(),
            Self::Previous => t::previous(),
            Self::Next => t::next(),
            Self::Like => t::like(),
            Self::Queue => t::queue(),
            Self::ThemeSystem => t::theme_system(),
            Self::ThemeDark => t::theme_dark(),
            Self::ThemeLight => t::theme_light(),
            Self::CheckForUpdates => i18n::update::check_for_updates(),
            Self::MiniPlayer => t::mini_player(),
        }
    }

    fn glyph(self) -> Icon {
        match self {
            Self::Home => Icon::Home,
            Self::Search => Icon::Search,
            Self::History => Icon::History,
            Self::Jam => Icon::Jam,
            Self::Account => Icon::Account,
            Self::Settings | Self::ThemeSystem | Self::CheckForUpdates => Icon::Settings,
            Self::Feed => Icon::Feed,
            Self::Likes | Self::Like => Icon::Heart,
            Self::Library => Icon::Library,
            Self::Following => Icon::People,
            Self::TogglePlay => Icon::Play,
            Self::Previous => Icon::Previous,
            Self::Next => Icon::Next,
            Self::Queue => Icon::Queue,
            Self::MiniPlayer => Icon::MiniPlayer,
            Self::ThemeDark => Icon::Moon,
            Self::ThemeLight => Icon::Sun,
        }
    }

    /// The key that does the same, when there is one.
    fn keys(self) -> Option<&'static str> {
        use i18n::shortcuts as k;
        match self {
            Self::Search => Some(shortcuts::platform(k::key_search(), k::key_search_mac())),
            Self::TogglePlay => Some(k::key_play()),
            Self::Like => Some(shortcuts::platform(k::key_like(), k::key_like_mac())),
            Self::MiniPlayer => Some(shortcuts::platform(k::key_mini(), k::key_mini_mac())),
            _ => None,
        }
    }
}

/// The items to offer now: the account's screens need an account, and
/// playback actions need a track.
pub(crate) fn available(signed_in: bool, has_track: bool) -> Vec<PaletteCommand> {
    use PaletteCommand::{Feed, Following, Library, Like, Likes, Next, Previous, TogglePlay};
    PaletteCommand::ALL
        .into_iter()
        .filter(|command| match command {
            Feed | Likes | Library | Following => signed_in,
            TogglePlay | Previous | Next | Like => has_track,
            _ => true,
        })
        .collect()
}

/// The items whose label fits what was typed.
pub(crate) fn filter(items: &[PaletteCommand], query: &str) -> Vec<PaletteCommand> {
    items
        .iter()
        .copied()
        .filter(|item| surface::matches(query, item.label()))
        .collect()
}

/// The palette while it is open.
pub(super) struct PaletteState {
    matches: Vec<PaletteCommand>,
    selected: usize,
    scroll: ScrollHandle,
}

/// Attaches the palette's handlers to the root.
pub(super) fn palette_actions(root: Div, cx: &mut Context<Shell>) -> Div {
    root.on_action(cx.listener(Shell::on_toggle_palette))
        .on_action(cx.listener(Shell::on_palette_up))
        .on_action(cx.listener(Shell::on_palette_down))
        .on_action(cx.listener(Shell::on_palette_confirm))
        .on_action(cx.listener(Shell::on_palette_close))
}

impl Shell {
    fn palette_items(&self) -> Vec<PaletteCommand> {
        available(self.models.account.is_some(), self.models.current.is_some())
    }

    /// The field's text changed: filter again and select the first row.
    pub(super) fn palette_query_changed(&mut self, query: &str, cx: &mut Context<Self>) {
        if self.palette.is_none() {
            return;
        }
        let matches = filter(&self.palette_items(), query);
        if let Some(state) = &mut self.palette {
            state.matches = matches;
            state.selected = 0;
            state.scroll.scroll_to_item(0);
        }
        cx.notify();
    }

    fn on_toggle_palette(
        &mut self,
        _: &TogglePalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.palette.is_some() {
            self.close_palette(window, cx);
            return;
        }
        // A dialog has the keyboard; the palette waits for it to close.
        if self.dialog.is_some() {
            return;
        }
        self.playlist_menu = None;
        self.palette = Some(PaletteState {
            matches: self.palette_items(),
            selected: 0,
            scroll: ScrollHandle::new(),
        });
        self.palette_field.update(cx, |field, cx| field.reset(cx));
        window.focus(&self.palette_field.focus_handle(cx), cx);
        cx.notify();
    }

    fn on_palette_close(&mut self, _: &PaletteClose, window: &mut Window, cx: &mut Context<Self>) {
        self.close_palette(window, cx);
    }

    fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    fn on_palette_up(&mut self, _: &PaletteUp, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(state) = &mut self.palette {
            state.selected = state.selected.saturating_sub(1);
            state.scroll.scroll_to_item(state.selected);
            cx.notify();
        }
    }

    fn on_palette_down(&mut self, _: &PaletteDown, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(state) = &mut self.palette {
            state.selected = (state.selected + 1).min(state.matches.len().saturating_sub(1));
            state.scroll.scroll_to_item(state.selected);
            cx.notify();
        }
    }

    fn on_palette_confirm(
        &mut self,
        _: &PaletteConfirm,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let chosen = self
            .palette
            .as_ref()
            .and_then(|state| state.matches.get(state.selected))
            .copied();
        if let Some(command) = chosen {
            self.run_palette_command(command, window, cx);
        }
    }

    /// Closes the palette, then does what the row says.
    fn run_palette_command(
        &mut self,
        command: PaletteCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_palette(window, cx);
        let me = self.models.account.as_ref().map(|account| account.id);
        match command {
            PaletteCommand::Home => self.navigate(Route::Home, cx),
            PaletteCommand::Search => window.focus(&self.search.focus_handle(cx), cx),
            PaletteCommand::History => self.dispatch(UiIntent::OpenHistory, cx),
            PaletteCommand::Jam => self.navigate(Route::Jam, cx),
            PaletteCommand::Account => self.navigate(Route::Account, cx),
            PaletteCommand::Settings => self.navigate(Route::Settings, cx),
            PaletteCommand::Feed => self.open_account_list(Route::Feed, cx),
            PaletteCommand::Library => self.open_account_list(Route::Library, cx),
            PaletteCommand::Likes => {
                if let Some(me) = me {
                    self.open_account_list(Route::Likes(me), cx);
                }
            }
            PaletteCommand::Following => {
                if let Some(me) = me {
                    self.open_account_list(Route::Following(me), cx);
                }
            }
            PaletteCommand::TogglePlay => self.send(Command::TogglePlay),
            PaletteCommand::Previous => self.send(Command::Previous),
            PaletteCommand::Next => self.send(Command::Next),
            PaletteCommand::Like => self.toggle_like(cx),
            PaletteCommand::Queue => self.toggle_queue(cx),
            PaletteCommand::ThemeSystem => {
                self.change_settings(|settings| settings.theme = ThemeChoice::System);
            }
            PaletteCommand::ThemeDark => {
                self.change_settings(|settings| settings.theme = ThemeChoice::Dark);
            }
            PaletteCommand::ThemeLight => {
                self.change_settings(|settings| settings.theme = ThemeChoice::Light);
            }
            PaletteCommand::CheckForUpdates => self.check_for_updates(true, cx),
            PaletteCommand::MiniPlayer => self.toggle_mini_player(cx),
        }
    }

    /// The palette over the window, when open.
    pub(super) fn palette_view(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.palette.as_ref()?;
        let rows = state
            .matches
            .iter()
            .enumerate()
            .map(|(ix, command)| {
                let command = *command;
                palette_row(
                    theme,
                    ("palette-row", ix),
                    command.glyph(),
                    command.label(),
                    command.keys(),
                    ix == state.selected,
                )
                .aria_label(command.label())
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.run_palette_command(command, window, cx);
                }))
                .into_any_element()
            })
            .collect();
        let empty = state.matches.is_empty().then(|| t::no_matches().into());
        Some(
            palette(
                theme,
                "palette",
                t::title().into(),
                self.palette_field.clone().into_any_element(),
                rows,
                empty,
                &state.scroll,
                cx.listener(|this, _, window, cx| this.close_palette(window, cx)),
            )
            .into_any_element(),
        )
    }

    /// Subscribes to the palette field; called once from `Shell::new`.
    pub(super) fn watch_palette_field(
        cx: &mut Context<Self>,
        field: &gpui::Entity<cloudrs_ui::search_field::SearchField>,
    ) {
        cx.subscribe(field, |this, _, event: &SearchChanged, cx| {
            this.palette_query_changed(&event.0, cx);
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_out_hides_account_screens() {
        let items = available(false, false);
        for hidden in [
            PaletteCommand::Feed,
            PaletteCommand::Likes,
            PaletteCommand::Library,
            PaletteCommand::Following,
        ] {
            assert!(!items.contains(&hidden), "{hidden:?}");
        }
        assert!(items.contains(&PaletteCommand::Settings));
    }

    #[test]
    fn playback_needs_a_track() {
        let none = available(true, false);
        let some = available(true, true);
        for command in [
            PaletteCommand::TogglePlay,
            PaletteCommand::Previous,
            PaletteCommand::Next,
            PaletteCommand::Like,
        ] {
            assert!(!none.contains(&command), "{command:?}");
            assert!(some.contains(&command), "{command:?}");
        }
        assert!(none.contains(&PaletteCommand::Queue));
    }

    #[test]
    fn checking_for_updates_is_always_offered() {
        assert!(available(false, false).contains(&PaletteCommand::CheckForUpdates));
        assert!(available(true, true).contains(&PaletteCommand::CheckForUpdates));
    }

    #[test]
    fn every_screen_is_reachable() {
        // The routes without parameters; the sidebar and the title bar reach
        // the same ones. A new `Route` must be added here or have a reason not to.
        let screens = [
            (Route::Home, PaletteCommand::Home),
            (Route::Search, PaletteCommand::Search),
            (Route::History, PaletteCommand::History),
            (Route::Jam, PaletteCommand::Jam),
            (Route::Account, PaletteCommand::Account),
            (Route::Settings, PaletteCommand::Settings),
            (Route::Feed, PaletteCommand::Feed),
            (Route::Library, PaletteCommand::Library),
            (Route::Likes(sc_core::UserId(1)), PaletteCommand::Likes),
            (
                Route::Following(sc_core::UserId(1)),
                PaletteCommand::Following,
            ),
        ];
        let items = available(true, false);
        for (route, command) in screens {
            assert!(items.contains(&command), "{route:?}");
        }
    }

    #[test]
    fn filter_is_case_insensitive() {
        let items = available(true, true);
        assert_eq!(filter(&items, "SETTINGS"), [PaletteCommand::Settings]);
        assert_eq!(filter(&items, ""), items);
        assert!(filter(&items, "zzz").is_empty());
        let dark = filter(&items, "dark");
        assert_eq!(dark, [PaletteCommand::ThemeDark]);
    }
}
