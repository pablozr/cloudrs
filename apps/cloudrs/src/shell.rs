//! The window's one view (ADR 0005). It owns the core handle, runs the only
//! event pump, keeps the search results and forwards player events to the
//! player bar, which stays a separate entity.

use std::ops::Range;
use std::time::Duration;

use cloudrs_ui::components::{
    ButtonKind, ToastKind, TrackRowData, button, row_action, skeleton_row, toast, track_row,
};
use cloudrs_ui::search_field::{SearchChanged, SearchField};
use cloudrs_ui::tokens::{size, space, typography};
use cloudrs_ui::{Theme, ThemeMode, motion};
use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, Entity, FocusHandle, Focusable, KeyBinding, Role, ScrollStrategy,
    Stateful, Task, UniformListScrollHandle, Window, actions, div, px, uniform_list,
};
use sc_core::{Command, CoreConfig, CoreHandle, Event, Problem, StartError};

use crate::i18n;
use crate::player_bar::{PlayerAction, PlayerBar};
use crate::state::{Phase, QueueState, ResultsState, format_time, is_soundcloud_url};

mod queue_panel;

actions!(shell, [FocusSearch]);

/// Key context of the root, so `/` can be limited to when no field is focused.
const CONTEXT: &str = "Shell";

/// How long closing the window waits for the core to save its state.
const SHUTDOWN_WAIT: Duration = Duration::from_millis(500);
const SHUTDOWN_POLL: Duration = Duration::from_millis(20);

/// Skeleton rows while a search runs, and at the end while the next page loads.
const SEARCH_SKELETONS: usize = 10;
const PAGE_SKELETONS: usize = 3;

/// Registers the shell's key bindings. Call once at startup.
pub fn bind_keys(cx: &mut App) {
    let focus_search = if cfg!(target_os = "macos") {
        "cmd-k"
    } else {
        "ctrl-k"
    };
    cx.bind_keys([
        KeyBinding::new(focus_search, FocusSearch, None),
        // "/" must stay typeable inside the field.
        KeyBinding::new("/", FocusSearch, Some("Shell && !SearchField")),
    ]);
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
    player: Entity<PlayerBar>,
    results: ResultsState,
    scroll: UniformListScrollHandle,
    queue: QueueState,
    queue_open: bool,
    queue_scroll: UniformListScrollHandle,
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
        cx.subscribe(&search, |this, _, event: &SearchChanged, _| {
            this.on_search(&event.0);
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

        window.focus(&search.focus_handle(cx), cx);

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
            player,
            results: ResultsState::new(),
            scroll: UniformListScrollHandle::new(),
            queue: QueueState::default(),
            queue_open: false,
            queue_scroll: UniformListScrollHandle::new(),
            toast: None,
            toast_timer: None,
            toasts_shown: 0,
            stopping: false,
            stopped: false,
        };
        shell.start_pump(cx);
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

    fn send(&self, command: Command) {
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
        // Results first: the player reads the artwork they collect.
        let changed = self.results.apply(&event);
        let queue_changed = self.queue.apply(&event);
        if matches!(&event, Event::Searching { .. }) {
            self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        }
        match &event {
            Event::NowPlaying(_)
            | Event::Waveform { .. }
            | Event::Playback(_)
            | Event::Queue(_)
            | Event::Artwork { .. } => {
                let artwork = &self.results.artwork;
                self.player
                    .update(cx, |bar, cx| bar.apply(&event, artwork, cx));
            }
            Event::Problem(problem) => self.show_problem(problem, cx),
            Event::Stopped => self.stopped = true,
            // A failed first page has its own error state; a failed next page
            // leaves the list in place, so it gets a toast.
            Event::SearchFailed {
                append: true,
                problem,
                ..
            } => self.show_problem(problem, cx),
            Event::SearchFailed { .. } | Event::Searching { .. } | Event::Results { .. } => {}
        }
        // Playback ticks leave both false: they must not re-render the list or
        // the queue panel. Only the play/pause flip (inside `changed`) does.
        let artwork_in_queue = self.queue_open && self.queue.shows_artwork_of(&event);
        if changed || artwork_in_queue || (queue_changed && self.queue_open) {
            cx.notify();
        }
    }

    fn show_problem(&mut self, problem: &Problem, cx: &mut Context<Self>) {
        use i18n::problem as t;
        let (kind, text) = match problem {
            Problem::Offline => (ToastKind::Warning, t::offline()),
            Problem::RateLimited => (ToastKind::Warning, t::rate_limited()),
            Problem::NotFound => (ToastKind::Error, t::not_found()),
            Problem::NotATrack => (ToastKind::Error, t::not_a_track()),
            Problem::PreviewOnly => (ToastKind::Info, t::preview_only()),
            Problem::CannotPlay => (ToastKind::Error, t::cannot_play()),
            Problem::StorageReset => (ToastKind::Warning, t::storage_reset()),
            Problem::Audio(detail) => {
                tracing::warn!(%detail, "audio problem");
                (ToastKind::Error, t::audio())
            }
        };
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

    fn on_search(&self, text: &str) {
        let text = text.trim();
        if is_soundcloud_url(text) {
            self.send(Command::PlayUrl(text.to_owned()));
        } else {
            self.send(Command::Search(text.to_owned()));
        }
    }

    fn retry_search(&mut self, _: &gpui::ClickEvent, _: &mut Window, _: &mut Context<Self>) {
        self.send(Command::Search(self.results.query.clone()));
    }

    fn restart_core(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.core = start_core(&self.config);
        self.start_pump(cx);
        cx.notify();
    }

    fn focus_search(&mut self, _: &FocusSearch, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.search.focus_handle(cx), cx);
    }

    /// The visible rows of the results list (and skeletons for the next page).
    fn rows(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if self.results.take_load_more(range.end) {
            self.send(Command::LoadMore);
            cx.notify();
        }
        let theme = Theme::of(cx);
        range
            .map(|ix| {
                let Some(track) = self.results.tracks.get(ix) else {
                    return skeleton_row(&theme, ("skeleton-more", ix)).into_any_element();
                };
                let id = track.id;
                let active = self.results.current == Some(id);
                let duration = format_time(track.duration);
                let row = TrackRowData {
                    index: ix + 1,
                    title: &track.title,
                    artist: &track.artist,
                    duration: &duration,
                    artwork: self.results.artwork.get(&id).cloned(),
                    active,
                    playing: active && self.results.playing,
                    preview_badge: track.preview_only.then(i18n::search::preview_badge),
                    actions: vec![
                        row_action(
                            &theme,
                            ("play-next", ix),
                            i18n::queue::play_next(),
                            cx.listener(move |this, _, _, _| this.send(Command::PlayNext(id))),
                        )
                        .into_any_element(),
                        row_action(
                            &theme,
                            ("add-to-queue", ix),
                            i18n::queue::add_to_queue(),
                            cx.listener(move |this, _, _, _| this.send(Command::AddToQueue(id))),
                        )
                        .into_any_element(),
                    ],
                };
                track_row(&theme, ("track", ix), row)
                    .aria_label(i18n::search::play_track(&track.title, &track.artist))
                    .on_click(cx.listener(move |this, _, _, _| this.send(Command::Play(id))))
                    .into_any_element()
            })
            .collect()
    }

    fn results_view(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        match self.results.phase {
            Phase::Empty => status_view(
                theme,
                "empty",
                i18n::search::empty_title(),
                i18n::search::empty_hint(),
                None,
            ),
            Phase::Searching => div()
                .flex()
                .flex_col()
                .px(space::S5)
                .pt(space::S2)
                .children((0..SEARCH_SKELETONS).map(|i| skeleton_row(theme, ("skeleton", i))))
                .into_any_element(),
            Phase::Failed => status_view(
                theme,
                "failed",
                i18n::search::error_title(),
                i18n::search::error_hint(),
                Some(
                    button(
                        theme,
                        "retry-search",
                        i18n::app::try_again(),
                        ButtonKind::Primary,
                    )
                    .on_click(cx.listener(Self::retry_search)),
                ),
            ),
            Phase::Ready if self.results.tracks.is_empty() => status_view(
                theme,
                "no-results",
                &i18n::search::no_results_title(&self.results.query),
                i18n::search::no_results_hint(),
                None,
            ),
            Phase::Ready => {
                let more = if self.results.loading_more {
                    PAGE_SKELETONS
                } else {
                    0
                };
                div()
                    .id("results")
                    .role(Role::List)
                    .aria_label(i18n::search::results())
                    .size_full()
                    .px(space::S5)
                    .child(
                        uniform_list(
                            "results-list",
                            self.results.tracks.len() + more,
                            cx.processor(|this, range, _, cx| this.rows(range, cx)),
                        )
                        .track_scroll(&self.scroll)
                        .size_full(),
                    )
                    .into_any_element()
            }
        }
    }

    fn header(&self, theme: &Theme) -> impl IntoElement {
        let c = theme.colors;
        let theme_button = button(
            theme,
            "theme-toggle",
            match theme.mode {
                ThemeMode::Dark => i18n::app::switch_to_light(),
                ThemeMode::Light => i18n::app::switch_to_dark(),
            },
            ButtonKind::Ghost,
        )
        .on_click(|_, window, cx| {
            let next = cx
                .try_global::<ThemeMode>()
                .copied()
                .unwrap_or_default()
                .toggled();
            cx.set_global(next);
            window.refresh();
        });
        div()
            .flex()
            .items_center()
            .gap(space::S6)
            .px(space::S7)
            .py(space::S4)
            .child(
                theme
                    .text(div(), typography::DISPLAY_L)
                    .flex()
                    .flex_none()
                    .text_color(c.text)
                    .child(i18n::app::brand_cloud())
                    .child(div().text_color(c.accent).child(i18n::app::brand_rs())),
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

/// A centered title and hint, with an optional action: the empty and error
/// states of a surface.
fn status_view(
    theme: &Theme,
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
            return root.child(status_view(&theme, "startup", title, hint, Some(retry)));
        }

        let results = self.results_view(&theme, cx);
        let queue_panel = self.queue_open.then(|| self.queue_panel(&theme, cx));
        let toast = self
            .toast
            .as_ref()
            .map(|t| toast(&theme, ("toast", t.id), t.kind, t.text));
        root.child(self.header(&theme))
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
                            .child(results)
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
            )
            .child(self.player.clone())
    }
}
