//! What plays, on Discord (ADR 0015). The shell follows the core's events
//! here and sends Discord a new presence only when something a person would
//! see changed: the track, play or pause, a seek, the Jam. Playback ticks
//! never reach Discord.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use sc_core::{Event, JamState, PlayState, TrackId, TrackSummary};
use sc_platform::discord::{Listening, Presence};

use super::Shell;
use crate::i18n::discord as t;

/// A start this far from the last one sent is a seek, worth an update.
const SEEK_THRESHOLD: Duration = Duration::from_secs(2);
/// The most people a Jam takes (ADR 0011 §7).
const JAM_MAX: u32 = 16;
/// Where cloudrs is to get.
const DOWNLOAD_URL: &str = "https://github.com/pablozr/cloudrs";

#[derive(Default)]
pub(crate) struct DiscordPresence {
    client: Option<Presence>,
    /// The person turned it off on the Account screen.
    pub(crate) off: bool,
    /// Where that choice is kept between runs (a file that exists when off).
    off_flag: PathBuf,
    now: Option<TrackSummary>,
    links: Option<(TrackId, Option<String>, Option<String>)>,
    playing: bool,
    position: Duration,
    jam_people: Option<u32>,
    sent: Option<Listening>,
}

impl DiscordPresence {
    pub(crate) fn new(data_dir: &std::path::Path) -> Self {
        let off_flag = data_dir.join("discord-off");
        Self {
            client: Presence::start(),
            off: off_flag.exists(),
            off_flag,
            ..Self::default()
        }
    }

    /// Whether Discord is available to this build (an application id is set).
    pub(crate) fn available(&self) -> bool {
        self.client.is_some()
    }

    fn listening(&self) -> Option<Listening> {
        let track = self.now.as_ref()?;
        let (cover_url, page_url) = match &self.links {
            Some((id, cover, page)) if *id == track.id => (cover.clone(), page.clone()),
            _ => (None, None),
        };
        let span = self.playing.then(|| {
            let start = SystemTime::now() - self.position;
            (start, start + track.duration)
        });
        let state = match self.jam_people {
            Some(people) => t::in_jam(&track.artist, people),
            None => t::by(&track.artist),
        };
        let mut buttons = Vec::new();
        if let Some(page) = &page_url {
            buttons.push((t::listen_on_soundcloud().to_owned(), page.clone()));
        }
        buttons.push((t::get_cloudrs().to_owned(), DOWNLOAD_URL.to_owned()));
        Some(Listening {
            details: track.title.clone(),
            state,
            page_url,
            cover_url,
            cover_text: track.artist.clone(),
            status_text: if self.playing {
                t::playing().to_owned()
            } else {
                t::paused().to_owned()
            },
            span,
            party: self.jam_people.map(|people| (people, JAM_MAX)),
            buttons,
        })
    }

    /// Sends the presence when it changed for the eye; a playing track's
    /// times drift a little every tick and only a seek counts.
    fn sync(&mut self) {
        let Some(client) = &self.client else {
            return;
        };
        let next = if self.off { None } else { self.listening() };
        let same = match (&self.sent, &next) {
            (None, None) => true,
            (Some(sent), Some(next)) => {
                let close = match (sent.span, next.span) {
                    (Some((a, _)), Some((b, _))) => a
                        .duration_since(b)
                        .or_else(|_| b.duration_since(a))
                        .is_ok_and(|gap| gap < SEEK_THRESHOLD),
                    (None, None) => true,
                    _ => false,
                };
                close
                    && Listening {
                        span: None,
                        ..sent.clone()
                    } == Listening {
                        span: None,
                        ..next.clone()
                    }
            }
            _ => false,
        };
        if !same {
            client.show(next.clone());
            self.sent = next;
        }
    }

    /// Turns showing on Discord on or off, and remembers it.
    pub(crate) fn set_off(&mut self, off: bool) {
        self.off = off;
        let kept = if off {
            std::fs::write(&self.off_flag, b"")
        } else {
            std::fs::remove_file(&self.off_flag).or(Ok(()))
        };
        if let Err(error) = kept {
            tracing::warn!(%error, "could not remember the Discord choice");
        }
        self.sync();
    }
}

impl Shell {
    /// Follows what Discord shows: the track, its links, play state and the Jam.
    pub(crate) fn discord_event(&mut self, event: &Event) {
        let presence = &mut self.discord;
        match event {
            Event::NowPlaying(track) => {
                presence.now = Some(track.clone());
                presence.position = Duration::ZERO;
            }
            Event::NowPlayingLinks {
                track,
                cover_url,
                page_url,
            } => presence.links = Some((*track, cover_url.clone(), page_url.clone())),
            Event::Playback(playback) => {
                presence.playing = playback.state == PlayState::Playing;
                presence.position = playback.position;
            }
            Event::Jam(jam) => {
                presence.jam_people = jam.as_ref().map(|j: &JamState| j.people.len() as u32 + 1);
            }
            _ => return,
        }
        presence.sync();
    }
}
