//! The window's one view (ADR 0005, ADR 0008). It owns the core handle, runs
//! the only event pump, keeps the view models and the router, draws the
//! sidebar and header around the current screen and forwards player events to
//! the player bar, which stays a separate entity.

use std::collections::HashMap;
use std::time::Duration;

use cloudrs_ui::assets;
use cloudrs_ui::browse::sidebar_item;
use cloudrs_ui::components::{
    ButtonKind, Icon, ToastKind, artwork_tint, button, icon, icon_button, toast, tooltip,
};
use cloudrs_ui::search_field::{SearchChanged, SearchField};
use cloudrs_ui::tokens::{self, size, space, typography};
use cloudrs_ui::{Theme, ThemeMode, motion};
use gpui::prelude::*;
use gpui::{
    AnimationExt, AnyElement, App, Context, Entity, FocusHandle, Focusable, KeyBinding,
    MouseButton, NavigationDirection, Role, ScrollStrategy, Stateful, Task,
    UniformListScrollHandle, Window, actions, div, img, px,
};
use sc_core::{ArtKey, Command, CoreConfig, CoreHandle, Event, Problem, StartError};

use crate::i18n;
use crate::intent::UiIntent;
use crate::models::{ListId, Models};
use crate::nav::{Route, Router, Section};
use crate::player_bar::{PlayerAction, PlayerBar};
use crate::screens::{TrackWave, WaveAction};
use crate::seam;
use crate::state::{QueueState, is_soundcloud_url};
use crate::tint::{self, Rgb};

mod queue_panel;

actions!(shell, [FocusSearch, GoBack, GoForward]);

/// Key context of the root, so `/` can be limited to when no field is focused.
const CONTEXT: &str = "Shell";

/// How long closing the window waits for the core to save its state.
const SHUTDOWN_WAIT: Duration = Duration::from_millis(500);
const SHUTDOWN_POLL: Duration = Duration::from_millis(20);

/// Registers the shell's key bindings. Call once at startup.
pub fn bind_keys(cx: &mut App) {
    let focus_search = if cfg!(target_os = "macos") {
        "cmd-k"
    } else {
        "ctrl-k"
    };
    cx.bind_keys([
        KeyBinding::new("alt-left", GoBack, None),
        KeyBinding::new("alt-right", GoForward, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-[", GoBack, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-]", GoForward, None),
        KeyBinding::new(focus_search, FocusSearch, None),
        // "/" must stay typeable inside the field.
        KeyBinding::new("/", FocusSearch, Some("Shell && !SearchField")),
    ]);
}

/// The page tint and the one it is fading from. `seq` changes with every
/// switch, so the cross-fade plays again.
#[derive(Default)]
struct TintFade {
    from: Option<Rgb>,
    to: Option<Rgb>,
    seq: usize,
}

struct ToastState {
    /// Changes with every toast, so its entrance animation plays again.
    id: usize,
    kind: ToastKind,
    text: &'static str,
}

pub struct Shell {
    config: CoreConfig,
    core: Result<CoreHandle, StartError>,
    /// The only reader of the core's events; dropping it stops the pump.
    pump: Option<Task<()>>,
    focus: FocusHandle,
    search: Entity<SearchField>,
    /// The fallback sign-in: a pasted `oauth_token`.
    pub(crate) token_field: Entity<SearchField>,
    player: Entity<PlayerBar>,
    pub(crate) models: Models,
    pub(crate) router: Router,
    /// Changes with every navigation, so the screen's entrance plays again.
    pub(crate) nav_seq: usize,
    /// One per list, so going back finds the scroll position it left.
    pub(crate) scrolls: HashMap<ListId, UniformListScrollHandle>,
    /// The profile tab showing (Tracks, Playlists, Likes).
    pub(crate) user_tab: usize,
    pub(crate) wave: Entity<TrackWave>,
    queue: QueueState,
    queue_open: bool,
    queue_scroll: UniformListScrollHandle,
    tint: TintFade,
    toast: Option<ToastState>,
    toast_timer: Option<Task<()>>,
    toasts_shown: usize,
    /// The window asked to close and the core was told to shut down.
    stopping: bool,
    /// The core answered `Stopped`.
    stopped: bool,
}

fn start_core(config: &CoreConfig) -> Result<CoreHandle, StartError> {
    sc_core::start(config.clone()).inspect_err(|error| {
        tracing::error!(%error, "the core could not start");
    })
}

impl Shell {
    pub fn new(config: CoreConfig, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let hint = if cfg!(target_os = "macos") {
            i18n::search::hint_mac()
        } else {
            i18n::search::hint()
        };
        let search = cx.new(|cx| SearchField::new(i18n::search::placeholder(), hint, cx));
        cx.subscribe(&search, |this, _, event: &SearchChanged, cx| {
            this.on_search(&event.0, cx);
        })
        .detach();
        cx.on_focus_in(&search.focus_handle(cx), window, |this, _, cx| {
            this.show_search(cx);
        })
        .detach();

        let token_field = cx.new(|cx| {
            SearchField::new(i18n::account::token_placeholder(), "", cx).with_icon(Icon::SignIn)
        });

        let wave = cx.new(|_| TrackWave::new());
        cx.subscribe(&wave, |this, _, action: &WaveAction, cx| match *action {
            WaveAction::Seek(to) => this.send(Command::Seek(to)),
            WaveAction::Play(track) => {
                // Not in any list on screen: the core falls back to the track alone.
                let list = ListId::Related(track);
                this.dispatch(UiIntent::Play { list, track }, cx);
            }
        })
        .detach();

        let player = cx.new(|_| PlayerBar::new());
        cx.subscribe(&player, |this, _, action: &PlayerAction, cx| {
            let command = match *action {
                PlayerAction::TogglePlay => Command::TogglePlay,
                PlayerAction::Previous => Command::Previous,
                PlayerAction::Next => Command::Next,
                PlayerAction::Seek(to) => Command::Seek(to),
                PlayerAction::SetVolume(volume) => Command::SetVolume(volume),
                PlayerAction::SetShuffle(on) => Command::SetShuffle(on),
                PlayerAction::SetRepeat(repeat) => Command::SetRepeat(repeat),
                PlayerAction::ToggleQueue => {
                    this.queue_open = !this.queue_open;
                    let open = this.queue_open;
                    this.player
                        .update(cx, |bar, cx| bar.set_queue_open(open, cx));
                    cx.notify();
                    return;
                }
            };
            this.send(command);
        })
        .detach();
        window.focus(&cx.focus_handle(), cx);

        let this = cx.weak_entity();
        window.on_window_should_close(cx, move |_, cx| {
            this.update(cx, |shell, cx| shell.request_shutdown(cx))
                .unwrap_or(true)
        });

        let mut shell = Self {
            core: start_core(&config),
            config,
            pump: None,
            focus: cx.focus_handle(),
            search,
            token_field,
            player,
            models: Models::new(),
            router: Router::new(),
            nav_seq: 0,
            scrolls: HashMap::new(),
            user_tab: 0,
            wave,
            queue: QueueState::default(),
            queue_open: false,
            queue_scroll: UniformListScrollHandle::new(),
            tint: TintFade::default(),
            toast: None,
            toast_timer: None,
            toasts_shown: 0,
            stopping: false,
            stopped: false,
        };
        shell.start_pump(cx);
        // Home opens first; Ctrl K or / reach the search field.
        window.focus(&shell.focus, cx);
        shell.refresh_home(cx);
        shell
    }

    /// Closing the window: the core saves the session first. Returns whether
    /// the window may close now; otherwise the app quits once the core replies
    /// (or after a short wait, so a stuck core never keeps the window open).
    fn request_shutdown(&mut self, cx: &mut Context<Self>) -> bool {
        if self.stopping {
            return false;
        }
        let Ok(core) = &self.core else {
            return true;
        };
        if !core.send(Command::Shutdown) {
            return true;
        }
        self.stopping = true;
        cx.spawn(async move |this, cx| {
            for _ in 0..(SHUTDOWN_WAIT.as_millis() / SHUTDOWN_POLL.as_millis()) {
                cx.background_executor().timer(SHUTDOWN_POLL).await;
                if this.update(cx, |shell, _| shell.stopped).unwrap_or(true) {
                    break;
                }
            }
            cx.update(|cx| cx.quit());
        })
        .detach();
        false
    }

    pub(crate) fn send(&self, command: Command) {
        if let Ok(core) = &self.core {
            core.send(command);
        }
    }

    /// Starts reading the core's events, if the core is running.
    fn start_pump(&mut self, cx: &mut Context<Self>) {
        self.pump = None;
        let Ok(core) = &self.core else {
            return;
        };
        let events = core.events().clone();
        self.pump = Some(cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv_async().await {
                if this
                    .update(cx, |this, cx| this.on_event(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    fn on_event(&mut self, event: Event, cx: &mut Context<Self>) {
        // Models first: the player reads the artwork they collect.
        let changed = seam::apply(&mut self.models, &event);
        let queue_changed = self.queue.apply(&event);
        if let Event::Searching { kind, .. } = &event
            && let Some(scroll) = self.scrolls.get(&ListId::Search { kind: *kind })
        {
            scroll.scroll_to_item(0, ScrollStrategy::Top);
        }
        if let Event::Artwork { key, path } = &event
            && self.models.art.begin_tint(*key)
        {
            self.compute_tint(*key, path.clone(), cx);
        }
        self.resolve_link(&event, cx);
        self.wave.update(cx, |wave, cx| wave.apply(&event, cx));
        match &event {
            Event::NowPlaying(_)
            | Event::Waveform { .. }
            | Event::Playback(_)
            | Event::Queue(_)
            | Event::Artwork { .. } => {
                let artwork = &self.models.art.tracks;
                self.player
                    .update(cx, |bar, cx| bar.apply(&event, artwork, cx));
            }
            Event::Problem(problem) => self.show_problem(problem, cx),
            Event::Stopped => self.stopped = true,
            Event::SignedIn(account) => {
                let token = account.token.clone();
                keychain(cx, move || sc_platform::keychain::save_token(&token));
                self.clear_token_field(cx);
                if *self.router.current() == Route::Home {
                    self.refresh_home(cx);
                }
            }
            Event::SignedOut => {
                keychain(cx, sc_platform::keychain::delete_token);
                // An account screen of the person who left shows nothing now.
                if self.router.current().account_list().is_some() {
                    self.navigate(Route::Account, cx);
                }
            }
            // A failed first page has its own error state; a failed next page
            // leaves the list in place, so it gets a toast.
            Event::ListFailed {
                append: true,
                problem,
                ..
            } => self.show_problem(problem, cx),
            Event::ListFailed { .. }
            | Event::Searching { .. }
            | Event::List { .. }
            | Event::TrackPage(_)
            | Event::UserPage(_)
            | Event::PlaylistPage(_)
            | Event::LikedIds(_)
            | Event::FollowedIds(_)
            | Event::Liked { .. }
            | Event::Followed { .. }
            // The Jam panel arrives with the Jam UI.
            | Event::Jam(_) => {}
        }
        // Playback ticks leave both false: they must not re-render the list or
        // the queue panel. Only the play/pause flip (inside `changed`) does.
        let artwork_in_queue = self.queue_open && self.queue.shows_artwork_of(&event);
        if changed || artwork_in_queue || (queue_changed && self.queue_open) {
            cx.notify();
        }
    }

    /// Finds the artwork's colour off the UI thread, then shows it.
    fn compute_tint(&self, key: ArtKey, path: std::path::PathBuf, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let color = cx
                .background_executor()
                .spawn(async move { tint::dominant_color(&path) })
                .await;
            this.update(cx, |this, cx| {
                this.models.art.set_tint(key, color);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The colour the page behind the content takes: its own artwork on a
    /// page, the playing track's on search and history, none otherwise.
    fn tint_target(&self) -> Option<Rgb> {
        let key = match self.router.current() {
            Route::Track(id) => ArtKey::Track(*id),
            Route::User(id) => ArtKey::User(*id),
            Route::Playlist(id) => ArtKey::Playlist(*id),
            Route::Home
            | Route::Search
            | Route::History
            | Route::Feed
            | Route::Likes(_)
            | Route::Library
            | Route::Following(_)
            | Route::Account
            | Route::Jam => ArtKey::Track(self.models.current?),
            Route::Resolving(_) => return None,
        };
        self.models.art.tint(key)
    }

    /// Starts a cross-fade when the target colour changed.
    fn sync_tint(&mut self) {
        let target = self.tint_target();
        if target != self.tint.to {
            self.tint.from = self.tint.to;
            self.tint.to = target;
            self.tint.seq += 1;
        }
    }

    /// The tint layers, behind the header and the screen: the old colour
    /// fading out and the new one fading in.
    fn tint_layers(&self, theme: &Theme) -> Vec<AnyElement> {
        let seq = self.tint.seq;
        let layer = |color: Option<Rgb>, name: &'static str, fade_in: bool| {
            color.map(|color| {
                artwork_tint(theme, color.into())
                    .with_animation((name, seq), motion::PAGE.animation(), move |layer, t| {
                        layer.opacity(if fade_in { t } else { 1.0 - t })
                    })
                    .into_any_element()
            })
        };
        [
            layer(self.tint.from, "tint-out", false),
            layer(self.tint.to, "tint-in", true),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    /// A pasted link is answered by the page it leads to, by the track
    /// starting to play, or by a problem: the loading step goes away and the
    /// page, if any, takes its place.
    fn resolve_link(&mut self, event: &Event, cx: &mut Context<Self>) {
        if !matches!(self.router.current(), Route::Resolving(_)) {
            return;
        }
        let answer = match event {
            Event::TrackPage(page) => Some(Some(Route::Track(page.track.id))),
            Event::UserPage(page) => Some(Some(Route::User(page.id))),
            Event::PlaylistPage(page) => Some(Some(Route::Playlist(page.id))),
            Event::NowPlaying(_) | Event::Problem(_) => Some(None),
            _ => None,
        };
        if let Some(route) = answer {
            self.router.resolve(route);
            self.route_changed(cx);
        }
    }

    fn show_problem(&mut self, problem: &Problem, cx: &mut Context<Self>) {
        use i18n::problem as t;
        let (kind, text) = match problem {
            Problem::Offline => (ToastKind::Warning, t::offline()),
            Problem::RateLimited => (ToastKind::Warning, t::rate_limited()),
            Problem::NotFound => (ToastKind::Error, t::not_found()),
            Problem::UnsupportedLink => (ToastKind::Error, t::unsupported_link()),
            Problem::PreviewOnly => (ToastKind::Info, t::preview_only()),
            Problem::CannotPlay => (ToastKind::Error, t::cannot_play()),
            Problem::StorageReset => (ToastKind::Warning, t::storage_reset()),
            Problem::SignInFailed => (ToastKind::Error, t::sign_in_failed()),
            Problem::SessionExpired => (ToastKind::Warning, t::session_expired()),
            Problem::SignInRequired => (ToastKind::Info, t::sign_in_required()),
            Problem::JamUnreachable => (ToastKind::Error, t::jam_unreachable()),
            Problem::JamBadLink => (ToastKind::Error, t::jam_bad_link()),
            Problem::JamEnded => (ToastKind::Info, t::jam_ended()),
            Problem::JamRemoved => (ToastKind::Info, t::jam_removed()),
            Problem::JamFull => (ToastKind::Warning, t::jam_full()),
            Problem::JamVersion => (ToastKind::Warning, t::jam_version()),
            Problem::JamNotAllowed => (ToastKind::Info, t::jam_not_allowed()),
            Problem::Audio(detail) => {
                tracing::warn!(%detail, "audio problem");
                (ToastKind::Error, t::audio())
            }
        };
        self.show_toast(kind, text, cx);
    }

    pub(crate) fn show_toast(
        &mut self,
        kind: ToastKind,
        text: &'static str,
        cx: &mut Context<Self>,
    ) {
        self.toasts_shown += 1;
        self.toast = Some(ToastState {
            id: self.toasts_shown,
            kind,
            text,
        });
        // Replacing the timer drops the previous one, so a newer problem
        // always gets its full time on screen.
        self.toast_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(motion::TOAST_LIFETIME).await;
            this.update(cx, |this, cx| {
                this.toast = None;
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// Opens the SoundCloud sign-in window (a child process) and signs in
    /// with the token it returns. Closing the window just stops waiting.
    pub(crate) fn sign_in_with_window(&mut self, cx: &mut Context<Self>) {
        if self.models.signing_in {
            return;
        }
        self.models.signing_in = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async {
                    sc_platform::sign_in::sign_in_with_window(i18n::account::signed_out_title())
                })
                .await;
            this.update(cx, |this, cx| match result {
                Ok(Some(token)) => this.dispatch(UiIntent::SignIn(token), cx),
                Ok(None) => {
                    this.models.signing_in = false;
                    cx.notify();
                }
                Err(error) => {
                    tracing::warn!(%error, "the sign-in window could not start");
                    this.models.signing_in = false;
                    this.show_toast(ToastKind::Error, i18n::problem::sign_in_window(), cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// The fallback: signs in with the token pasted in the account screen.
    pub(crate) fn sign_in_with_token(&mut self, cx: &mut Context<Self>) {
        let token = self.token_field.read(cx).value().trim().to_owned();
        if token.is_empty() || self.models.signing_in {
            return;
        }
        self.dispatch(UiIntent::SignIn(token), cx);
    }

    fn clear_token_field(&mut self, cx: &mut Context<Self>) {
        self.token_field.update(cx, |field, cx| field.reset(cx));
    }

    /// Typing in the search field shows the search screen, then searches (or
    /// opens a pasted link).
    fn on_search(&mut self, text: &str, cx: &mut Context<Self>) {
        self.show_search(cx);
        let text = text.trim();
        let intent = if sc_core::is_jam_link(text) {
            UiIntent::JoinJam(text.to_owned())
        } else if is_soundcloud_url(text) {
            UiIntent::OpenUrl(text.to_owned())
        } else {
            UiIntent::Search(text.to_owned())
        };
        self.dispatch(intent, cx);
    }

    /// The one place intents go: opens the screen they lead to, updates the
    /// models and sends the core command, when the core can take it.
    pub(crate) fn dispatch(&mut self, intent: UiIntent, cx: &mut Context<Self>) {
        if let Some(route) = intent.route() {
            self.navigate(route, cx);
        }
        let command = seam::take(&mut self.models, &intent);
        self.send(command);
        cx.notify();
    }

    pub(crate) fn navigate(&mut self, route: Route, cx: &mut Context<Self>) {
        if self.router.push(route) {
            self.user_tab = 0;
            self.route_changed(cx);
        }
    }

    fn show_search(&mut self, cx: &mut Context<Self>) {
        self.navigate(Route::Search, cx);
    }

    fn go_back(&mut self, cx: &mut Context<Self>) {
        if self.router.back() {
            self.route_changed(cx);
        }
    }

    fn go_forward(&mut self, cx: &mut Context<Self>) {
        if self.router.forward() {
            self.route_changed(cx);
        }
    }

    /// After any change of route: replays the entrance and points the large
    /// waveform at the track page, if that is where we are.
    pub(crate) fn route_changed(&mut self, cx: &mut Context<Self>) {
        self.nav_seq += 1;
        let track = match self.router.current() {
            Route::Track(id) => Some(*id),
            _ => None,
        };
        self.wave.update(cx, |wave, cx| wave.show(track, cx));
        if *self.router.current() == Route::Home {
            self.refresh_home(cx);
        }
        cx.notify();
    }

    fn on_go_back(&mut self, _: &GoBack, _: &mut Window, cx: &mut Context<Self>) {
        self.go_back(cx);
    }

    fn on_go_forward(&mut self, _: &GoForward, _: &mut Window, cx: &mut Context<Self>) {
        self.go_forward(cx);
    }

    fn restart_core(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.core = start_core(&self.config);
        self.start_pump(cx);
        cx.notify();
    }

    pub(crate) fn focus_search_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.search.focus_handle(cx), cx);
    }

    fn focus_search(&mut self, _: &FocusSearch, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.search.focus_handle(cx), cx);
    }

    fn sidebar(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme.colors;
        let section = self.router.current().section();
        div()
            .id("sidebar")
            .role(Role::Navigation)
            .aria_label(i18n::nav::sidebar())
            .w(size::SIDEBAR_WIDTH)
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .gap(space::S1)
            .px(space::S3)
            .py(space::S4)
            .bg(c.canvas_deep)
            .border_r_1()
            .border_color(c.line)
            .child(
                theme
                    .text(div(), typography::DISPLAY_L)
                    .flex()
                    .items_center()
                    .gap(size::LOGO_CLEAR_SPACE)
                    .px(space::S4)
                    .pb(space::S6)
                    .text_color(c.text)
                    // Full colour: the symbol keeps its own gradient, so it is an
                    // image, not a tinted icon.
                    .child(img(assets::LOGO).size(size::LOGO).flex_none())
                    .child(
                        div()
                            .flex()
                            .child(i18n::app::brand_cloud())
                            .child(div().text_color(c.accent).child(i18n::app::brand_rs())),
                    ),
            )
            .child(
                sidebar_item(
                    theme,
                    "nav-home",
                    Icon::Home,
                    i18n::nav::home(),
                    section == Section::Home,
                )
                .aria_label(i18n::nav::home())
                .on_click(cx.listener(|this, _, _, cx| this.navigate(Route::Home, cx))),
            )
            .child(
                sidebar_item(
                    theme,
                    "nav-search",
                    Icon::Search,
                    i18n::nav::search(),
                    section == Section::Search,
                )
                .aria_label(i18n::nav::search())
                .on_click(cx.listener(|this, _, _, cx| this.show_search(cx))),
            )
            .child(
                sidebar_item(
                    theme,
                    "nav-history",
                    Icon::History,
                    i18n::nav::history(),
                    section == Section::History,
                )
                .aria_label(i18n::nav::history())
                .on_click(cx.listener(|this, _, _, cx| {
                    this.dispatch(UiIntent::OpenHistory, cx);
                })),
            )
            .child(
                sidebar_item(
                    theme,
                    "nav-jam",
                    Icon::Jam,
                    i18n::nav::jam(),
                    section == Section::Jam,
                )
                .aria_label(i18n::nav::jam())
                .on_click(cx.listener(|this, _, _, cx| this.navigate(Route::Jam, cx))),
            )
            .children(self.models.account.as_ref().map(|me| {
                let me = me.id;
                let items = [
                    (Route::Feed, Section::Feed, Icon::Feed, i18n::nav::feed()),
                    (
                        Route::Likes(me),
                        Section::Likes,
                        Icon::Heart,
                        i18n::nav::likes(),
                    ),
                    (
                        Route::Library,
                        Section::Library,
                        Icon::Library,
                        i18n::nav::library(),
                    ),
                    (
                        Route::Following(me),
                        Section::Following,
                        Icon::People,
                        i18n::nav::following(),
                    ),
                ];
                div()
                    .flex()
                    .flex_col()
                    .gap(space::S1)
                    .pt(space::S4)
                    .children(items.into_iter().map(|(route, item, glyph, label)| {
                        sidebar_item(theme, label, glyph, label, section == item)
                            .aria_label(label)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_account_list(route.clone(), cx);
                            }))
                    }))
            }))
            .child(div().flex_1())
            .child({
                let label = self
                    .models
                    .account
                    .as_ref()
                    .map_or(i18n::nav::sign_in().to_owned(), |me| me.username.clone());
                sidebar_item(
                    theme,
                    "nav-account",
                    Icon::Account,
                    label.clone(),
                    section == Section::Account,
                )
                .aria_label(label)
                .on_click(cx.listener(|this, _, _, cx| this.navigate(Route::Account, cx)))
            })
    }

    /// Opens Feed, Likes, Library or Following; the list loads the first time.
    fn open_account_list(&mut self, route: Route, cx: &mut Context<Self>) {
        let Some(list) = route.account_list() else {
            return;
        };
        self.navigate(route, cx);
        if self.models.lists.contains_key(&list) {
            cx.notify();
        } else {
            self.dispatch(UiIntent::OpenList(list), cx);
        }
    }

    fn header(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        // The icon shows what a click switches to.
        let (toggle_icon, toggle_label) = match theme.mode {
            ThemeMode::Dark => (Icon::Sun, i18n::app::switch_to_light()),
            ThemeMode::Light => (Icon::Moon, i18n::app::switch_to_dark()),
        };
        let theme_button = icon_button(theme, "theme-toggle", toggle_icon, false)
            .aria_label(toggle_label)
            .tooltip(tooltip(toggle_label))
            .on_click(|_, window, cx| {
                let next = cx
                    .try_global::<ThemeMode>()
                    .copied()
                    .unwrap_or_default()
                    .toggled();
                cx.set_global(next);
                window.refresh();
            });
        let dim = |enabled: bool| {
            if enabled {
                1.0
            } else {
                tokens::DISABLED_OPACITY
            }
        };
        let (can_back, can_forward) = (self.router.can_back(), self.router.can_forward());
        let back = icon_button(theme, "nav-back", Icon::Back, false)
            .aria_label(i18n::nav::back())
            .tooltip(tooltip(i18n::nav::back()))
            .opacity(dim(can_back))
            .when(can_back, |button| {
                button.on_click(cx.listener(|this, _, _, cx| this.go_back(cx)))
            });
        let forward = icon_button(theme, "nav-forward", Icon::Forward, false)
            .aria_label(i18n::nav::forward())
            .tooltip(tooltip(i18n::nav::forward()))
            .opacity(dim(can_forward))
            .when(can_forward, |button| {
                button.on_click(cx.listener(|this, _, _, cx| this.go_forward(cx)))
            });
        div()
            .flex()
            .items_center()
            .gap(space::S4)
            .px(space::S5)
            .py(space::S4)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .gap(space::S1)
                    .child(back)
                    .child(forward),
            )
            .child(
                div().flex().flex_1().justify_center().child(
                    div()
                        .w_full()
                        .max_w(size::SEARCH_MAX_WIDTH)
                        .child(self.search.clone()),
                ),
            )
            .child(theme_button)
    }
}

/// Keychain calls can wait on the OS (Secret Service on Linux), so they run
/// off the UI thread.
fn keychain(cx: &mut Context<Shell>, work: impl FnOnce() + Send + 'static) {
    cx.background_executor()
        .spawn(async move { work() })
        .detach();
}

/// A centered title and hint, with an optional action: the empty and error
/// states of a surface.
pub(crate) fn status_view(
    theme: &Theme,
    glyph: Icon,
    key: &'static str,
    title: &str,
    hint: &str,
    action: Option<Stateful<gpui::Div>>,
) -> AnyElement {
    let c = theme.colors;
    let view = div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(space::S2)
        .child(
            div()
                .pb(space::S2)
                .child(icon(glyph, size::ICON_STATUS, c.text_subtle)),
        )
        .child(
            theme
                .text(div(), typography::TITLE)
                .text_color(c.text)
                .child(title.to_owned()),
        )
        .child(
            theme
                .text(div(), typography::BODY_MUTED)
                .text_color(c.text_muted)
                .child(hint.to_owned()),
        )
        .when_some(action, |view, action| {
            view.child(div().pt(space::S4).child(action))
        });
    motion::content_in(key, view).into_any_element()
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let root = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.colors.canvas)
            .text_color(theme.colors.text)
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::focus_search));

        if let Err(error) = &self.core {
            let (title, hint) = match error {
                StartError::Audio(_) => (i18n::startup::audio_title(), i18n::startup::audio_hint()),
                StartError::Network(_) => (
                    i18n::startup::network_title(),
                    i18n::startup::network_hint(),
                ),
            };
            let retry = button(
                &theme,
                "restart-core",
                i18n::app::try_again(),
                ButtonKind::Primary,
            )
            .on_click(cx.listener(Self::restart_core));
            return root.child(status_view(
                &theme,
                Icon::Alert,
                "startup",
                title,
                hint,
                Some(retry),
            ));
        }

        self.sync_tint();
        let tint_layers = self.tint_layers(&theme);
        let screen = self.screen(&theme, cx);
        let queue_panel = self
            .queue_open
            .then(|| self.queue_panel(&theme, cx).into_any_element());
        let toast = self
            .toast
            .as_ref()
            .map(|t| toast(&theme, ("toast", t.id), t.kind, t.text));
        let sidebar = self.sidebar(&theme, cx).into_any_element();
        let header = self.header(&theme, cx).into_any_element();
        root.on_action(cx.listener(Self::on_go_back))
            .on_action(cx.listener(Self::on_go_forward))
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Back),
                cx.listener(|this, _, _, cx| this.go_back(cx)),
            )
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Forward),
                cx.listener(|this, _, _, cx| this.go_forward(cx)),
            )
            .child(
                div().flex().flex_1().min_h(px(0.0)).child(sidebar).child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.0))
                        .relative()
                        .children(tint_layers)
                        .child(header)
                        .child(
                            div()
                                .flex()
                                .flex_1()
                                .min_h(px(0.0))
                                .child(
                                    div()
                                        .relative()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .overflow_hidden()
                                        .child(screen)
                                        .when_some(toast, |body, toast| {
                                            body.child(
                                                div()
                                                    .absolute()
                                                    .bottom(space::S4)
                                                    .w_full()
                                                    .flex()
                                                    .justify_center()
                                                    .child(toast),
                                            )
                                        }),
                                )
                                .children(queue_panel),
                        ),
                ),
            )
            .child(self.player.clone())
    }
}
