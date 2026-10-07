//! Discord Rich Presence (ADR 0015): what plays shows on the person's
//! Discord profile, as "Listening to" with the cover, the artist, a progress
//! bar and buttons. It talks to the Discord app on this machine over its
//! local IPC; nothing goes through a server of ours.
//!
//! A thread owns the connection. When Discord is not running it tries again
//! every 30 s, quietly; the app only sends what to show.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use discord_rich_presence::activity::{
    Activity, ActivityType, Assets, Button, Party, StatusDisplayType, Timestamps,
};
use discord_rich_presence::{DiscordIpc, DiscordIpcClient};

/// The "cloudrs" application registered on Discord's developer portal. Its
/// name is what Discord shows ("Listening to cloudrs" in short views); the id
/// is public (empty would keep the presence off).
pub const APP_ID: &str = "1557535157018427493";

/// The small image: the logo uploaded to the application's Rich Presence
/// assets under this name.
const LOGO_ASSET: &str = "logo";
/// How long to wait before trying Discord again.
const RETRY: Duration = Duration::from_secs(30);

/// What to show, already in the person's language.
#[derive(Debug, Clone, PartialEq)]
pub struct Listening {
    /// The track's title (the first line).
    pub details: String,
    /// The second line: the artist, or the Jam ("In a Jam · 3 listening").
    pub state: String,
    /// The track's page, opened from its title.
    pub page_url: Option<String>,
    /// The cover (an https URL); the logo when there is none.
    pub cover_url: Option<String>,
    /// Hover text of the cover (the artist, or the album).
    pub cover_text: String,
    /// Hover text of the small logo ("Playing", "Paused").
    pub status_text: String,
    /// When the track started and ends, for Discord's progress bar; `None`
    /// while paused.
    pub span: Option<(SystemTime, SystemTime)>,
    /// In a Jam: people listening and the most a Jam takes.
    pub party: Option<(u32, u32)>,
    /// Up to two (label, https URL).
    pub buttons: Vec<(String, String)>,
}

/// The app's handle. Dropping it clears the presence and ends the thread.
pub struct Presence {
    updates: flume::Sender<Option<Listening>>,
}

impl Presence {
    /// Starts the presence thread, or `None` while no application id is set.
    pub fn start() -> Option<Self> {
        if APP_ID.is_empty() {
            return None;
        }
        let (updates, rx) = flume::unbounded();
        let spawned = std::thread::Builder::new()
            .name("cloudrs-discord".into())
            .spawn(move || run(&rx));
        if let Err(error) = spawned {
            tracing::warn!(%error, "the Discord presence thread could not start");
            return None;
        }
        Some(Self { updates })
    }

    /// Shows this on Discord, or clears it with `None`.
    pub fn show(&self, listening: Option<Listening>) {
        let _ = self.updates.send(listening);
    }
}

fn run(updates: &flume::Receiver<Option<Listening>>) {
    let mut client = DiscordIpcClient::new(APP_ID);
    let mut connected = false;
    let mut current: Option<Listening> = None;
    loop {
        match updates.recv_timeout(RETRY) {
            Ok(update) => current = update,
            // Nothing new: only a lost connection needs another try.
            Err(flume::RecvTimeoutError::Timeout) if connected => continue,
            Err(flume::RecvTimeoutError::Timeout) => {}
            Err(flume::RecvTimeoutError::Disconnected) => break,
        }
        if !connected {
            connected = client.connect().is_ok();
            if !connected {
                continue;
            }
        }
        let sent = match &current {
            Some(listening) => client.set_activity(activity(listening)),
            None => client.clear_activity(),
        };
        if sent.is_err() {
            // Discord closed: the next update or the retry connects again.
            let _ = client.close();
            connected = false;
        }
    }
    if connected {
        let _ = client.clear_activity();
        let _ = client.close();
    }
}

fn millis(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

fn activity(listening: &Listening) -> Activity<'_> {
    let mut assets = Assets::new()
        .large_image(listening.cover_url.as_deref().unwrap_or(LOGO_ASSET))
        .large_text(listening.cover_text.as_str())
        .small_image(LOGO_ASSET)
        .small_text(listening.status_text.as_str());
    if let Some(url) = &listening.page_url {
        assets = assets.large_url(url.as_str());
    }
    let mut activity = Activity::new()
        .activity_type(ActivityType::Listening)
        // The member list reads "Listening to <track>", not "to cloudrs".
        .status_display_type(StatusDisplayType::Details)
        .details(listening.details.as_str())
        .state(listening.state.as_str())
        .assets(assets);
    if let Some(url) = &listening.page_url {
        activity = activity.details_url(url.as_str());
    }
    if let Some((start, end)) = listening.span {
        activity = activity.timestamps(Timestamps::new().start(millis(start)).end(millis(end)));
    }
    if let Some((people, most)) = listening.party {
        activity = activity.party(
            Party::new()
                .id("cloudrs-jam")
                .size([people as i32, most as i32]),
        );
    }
    let buttons: Vec<Button> = listening
        .buttons
        .iter()
        .take(2)
        .map(|(label, url)| Button::new(label.as_str(), url.as_str()))
        .collect();
    if !buttons.is_empty() {
        activity = activity.buttons(buttons);
    }
    activity
}
