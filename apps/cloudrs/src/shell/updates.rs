//! Updates in the background (ADR 0026). `sc_platform::update` checks GitHub,
//! downloads and verifies on a thread of its own; the shell only starts it,
//! shows what it reports and installs the result when the app restarts or
//! quits. The core is not involved beyond storing the `auto_update` setting.

use std::path::Path;
use std::time::SystemTime;

use cloudrs_ui::components::ToastKind;
use gpui::{Context, Subscription, Task};
use sc_platform::update::{self, Config, Prepared, Progress};

use super::{Shell, ToastAction};
use crate::i18n::update as t;

/// Where the update stands, for Settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UpdateState {
    Idle,
    Checking,
    UpToDate,
    /// A newer version this copy cannot install itself (a `.deb`).
    Manual {
        version: String,
    },
    Downloading {
        version: String,
        percent: u8,
    },
    /// Downloaded and verified; installs on restart or quit.
    Ready(Prepared),
    Failed,
}

pub(crate) struct Updates {
    /// `None` in debug builds and while the public key is empty.
    config: Option<Config>,
    pub(crate) state: UpdateState,
    pub(crate) last_check: Option<SystemTime>,
    /// The person asked for the running check, so it reports with a toast.
    manual: bool,
    /// The quit in progress is a "Restart to update", so the new version starts.
    relaunch: bool,
    _ticker: Option<Task<()>>,
    _reader: Option<Task<()>>,
    _on_quit: Option<Subscription>,
}

impl Updates {
    pub(crate) fn new(cache_dir: &Path) -> Self {
        Self {
            config: Config::for_this_app(env!("CARGO_PKG_VERSION"), cache_dir.join("updates")),
            state: UpdateState::Idle,
            last_check: None,
            manual: false,
            relaunch: false,
            _ticker: None,
            _reader: None,
            _on_quit: None,
        }
    }

    /// Whether this build can update itself.
    pub(crate) fn available(&self) -> bool {
        self.config.is_some()
    }
}

/// What a report changes: the new state, and a toast when one is called for.
/// An automatic check only speaks up when there is something to install.
fn settle(
    progress: Progress,
    manual: bool,
) -> (
    UpdateState,
    Option<(ToastKind, String, Option<ToastAction>)>,
) {
    match progress {
        Progress::UpToDate => (
            UpdateState::UpToDate,
            manual.then(|| (ToastKind::Info, t::up_to_date().to_owned(), None)),
        ),
        Progress::Available { version } => {
            let notice = manual.then(|| (ToastKind::Info, t::available(&version), None));
            (UpdateState::Manual { version }, notice)
        }
        Progress::Downloading { version, percent } => {
            (UpdateState::Downloading { version, percent }, None)
        }
        Progress::Ready(prepared) => {
            let notice = (
                ToastKind::Info,
                t::ready(&prepared.version),
                Some(ToastAction::RestartToUpdate),
            );
            (UpdateState::Ready(prepared), Some(notice))
        }
        Progress::Failed(failure) => {
            if !manual {
                tracing::warn!(?failure, "the automatic update check failed");
            }
            (
                UpdateState::Failed,
                manual.then(|| (ToastKind::Error, t::failed().to_owned(), None)),
            )
        }
    }
}

/// The line Settings shows for a state; `checked_at` is the local time of the
/// last check, when known.
pub(crate) fn status_text(state: &UpdateState, checked_at: Option<String>) -> String {
    match state {
        UpdateState::Idle => t::never_checked().to_owned(),
        UpdateState::Checking => t::checking().to_owned(),
        UpdateState::UpToDate => match checked_at {
            Some(time) => t::checked_at(time),
            None => t::up_to_date().to_owned(),
        },
        UpdateState::Manual { version } => t::available(version),
        UpdateState::Downloading { version, percent } => t::downloading(version, percent),
        UpdateState::Ready(prepared) => t::ready(&prepared.version),
        UpdateState::Failed => t::failed().to_owned(),
    }
}

/// "14:05" in the person's time zone, or `None` where the system will not say
/// (some Unix setups refuse it in a multi-threaded process).
pub(crate) fn local_time(at: SystemTime) -> Option<String> {
    let offset = time::UtcOffset::current_local_offset().ok()?;
    let local = time::OffsetDateTime::from(at).to_offset(offset);
    Some(format!("{:02}:{:02}", local.hour(), local.minute()))
}

impl Shell {
    /// Starts the daily schedule and the install-on-quit hook. Does nothing in
    /// a build that cannot update itself.
    pub(super) fn start_updates(&mut self, cx: &mut Context<Self>) {
        if !self.updates.available() {
            return;
        }
        self.updates._on_quit = Some(cx.on_app_quit(|this, _| {
            this.apply_update_on_exit();
            async {}
        }));
        self.updates._ticker = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(update::START_DELAY).await;
            loop {
                let alive = this.update(cx, |shell, cx| {
                    let due = update::due(shell.updates.last_check, SystemTime::now());
                    if shell.models.settings.auto_update && due {
                        shell.check_for_updates(false, cx);
                    }
                });
                if alive.is_err() {
                    break;
                }
                cx.background_executor().timer(update::TICK).await;
            }
        }));
    }

    /// Looks for an update now (`manual`: the person asked, so say how it went).
    pub(crate) fn check_for_updates(&mut self, manual: bool, cx: &mut Context<Self>) {
        match &self.updates.state {
            UpdateState::Checking => {
                self.updates.manual |= manual;
                return;
            }
            UpdateState::Downloading { .. } => return,
            // The download waits for the restart; a new check would clear it.
            UpdateState::Ready(prepared) => {
                if manual {
                    let text = t::ready(&prepared.version);
                    let action = Some(ToastAction::RestartToUpdate);
                    self.show_toast_action(ToastKind::Info, text, action, cx);
                }
                return;
            }
            _ => {}
        }
        let Some(config) = self.updates.config.clone() else {
            if manual {
                self.show_toast(ToastKind::Info, t::unavailable(), cx);
            }
            return;
        };
        self.updates.state = UpdateState::Checking;
        self.updates.last_check = Some(SystemTime::now());
        self.updates.manual = manual;
        let rx = update::check(config);
        self.updates._reader = Some(cx.spawn(async move |this, cx| {
            while let Ok(progress) = rx.recv_async().await {
                if this
                    .update(cx, |shell, cx| shell.on_update_progress(progress, cx))
                    .is_err()
                {
                    break;
                }
            }
        }));
        cx.notify();
    }

    fn on_update_progress(&mut self, progress: Progress, cx: &mut Context<Self>) {
        let (state, notice) = settle(progress, self.updates.manual);
        self.updates.state = state;
        if let Some((kind, text, action)) = notice {
            self.show_toast_action(kind, text, action, cx);
        }
        cx.notify();
    }

    /// "Restart to update": closes like the window does (the core saves the
    /// session first), then the update installs and the new version starts.
    pub(crate) fn restart_to_update(&mut self, cx: &mut Context<Self>) {
        self.updates.relaunch = true;
        if self.request_shutdown(cx) {
            cx.quit();
        }
    }

    /// Runs as the app quits. A plain quit installs without relaunching, unless
    /// the person turned automatic updates off since the download.
    fn apply_update_on_exit(&mut self) {
        let relaunch = self.updates.relaunch;
        if !relaunch && !self.models.settings.auto_update {
            return;
        }
        if let UpdateState::Ready(prepared) = &self.updates.state
            && let Err(error) = prepared.apply(relaunch)
        {
            tracing::error!(%error, "could not install the update");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_platform::update::Failure;

    fn states() -> Vec<UpdateState> {
        vec![
            UpdateState::Idle,
            UpdateState::Checking,
            UpdateState::UpToDate,
            UpdateState::Manual {
                version: "1.2.3".into(),
            },
            UpdateState::Downloading {
                version: "1.2.3".into(),
                percent: 40,
            },
            UpdateState::Failed,
        ]
    }

    #[test]
    fn every_state_has_a_text() {
        for state in states() {
            assert!(!status_text(&state, None).is_empty(), "{state:?}");
            assert!(
                !status_text(&state, Some("14:05".into())).is_empty(),
                "{state:?}"
            );
        }
    }

    #[test]
    fn the_texts_name_the_version_and_the_time() {
        let downloading = UpdateState::Downloading {
            version: "1.2.3".into(),
            percent: 40,
        };
        assert_eq!(
            status_text(&downloading, None),
            "Downloading 1.2.3\u{2026} 40%"
        );
        assert!(status_text(&UpdateState::UpToDate, Some("14:05".into())).contains("14:05"));
    }

    #[test]
    fn an_automatic_check_stays_quiet_unless_it_found_an_install() {
        for progress in [
            Progress::UpToDate,
            Progress::Available {
                version: "1.2.3".into(),
            },
            Progress::Failed(Failure::Check),
            Progress::Failed(Failure::Download),
        ] {
            assert!(settle(progress, false).1.is_none());
        }
    }

    #[test]
    fn a_manual_check_always_says_how_it_went() {
        for progress in [
            Progress::UpToDate,
            Progress::Available {
                version: "1.2.3".into(),
            },
            Progress::Failed(Failure::Check),
        ] {
            assert!(settle(progress, true).1.is_some());
        }
    }

    #[test]
    fn progress_while_downloading_has_no_toast() {
        let progress = Progress::Downloading {
            version: "1.2.3".into(),
            percent: 5,
        };
        let (state, notice) = settle(progress, true);
        assert!(matches!(state, UpdateState::Downloading { percent: 5, .. }));
        assert!(notice.is_none());
    }

    #[test]
    fn a_failure_in_the_background_is_not_a_toast_but_is_a_state() {
        let (state, notice) = settle(Progress::Failed(Failure::Download), false);
        assert_eq!(state, UpdateState::Failed);
        assert!(notice.is_none());
    }
}
