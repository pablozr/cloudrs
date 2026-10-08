//! Saving and restoring the session, and recording the history (ADR 0007).

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use sc_api::SoundCloudApi;

use super::{Core, Input};
use crate::store::{self, Session, SessionTrack};
use crate::types::{ArtKey, PlayState, Problem, TrackSummary};
use crate::{Event, artwork};

/// How long after the last volume change the session is saved.
const VOLUME_SAVE_DELAY: Duration = Duration::from_secs(1);

/// The connection plus the sequence of the last session written, so a slow
/// older write never overwrites a newer one.
pub(super) struct Store {
    pub(super) conn: rusqlite::Connection,
    pub(super) last_seq: u64,
    /// The same guard for the settings row.
    pub(super) settings_seq: u64,
}

pub(super) type SharedStore = Arc<Mutex<Store>>;

/// An open store, the session it held, and whether a damaged file was reset.
pub(super) type OpenedStore = (SharedStore, Option<Session>, bool);

/// Opens the database and loads the saved session. Runs on a blocking thread.
pub(super) fn open_store(dir: &std::path::Path) -> Option<OpenedStore> {
    if let Err(error) = std::fs::create_dir_all(dir) {
        tracing::warn!(%error, "no session database; continuing without saving");
        return None;
    }
    let (conn, reset) = match store::open_or_reset(&dir.join(store::FILE_NAME)) {
        Ok(opened) => opened,
        Err(error) => {
            tracing::warn!(%error, "no session database; continuing without saving");
            return None;
        }
    };
    let session = store::load_session(&conn)
        .inspect_err(|error| tracing::warn!(%error, "could not read the saved session"))
        .ok()
        .flatten();
    Some((
        Arc::new(Mutex::new(Store {
            conn,
            last_seq: 0,
            settings_seq: 0,
        })),
        session,
        reset,
    ))
}

impl<A: SoundCloudApi + 'static> Core<A> {
    /// The state worth keeping across a restart.
    pub(super) fn session(&self) -> Session {
        // A Jam guest's own session, not the host's queue it mirrors.
        if let Some(saved) = self.jam_saved_session() {
            return saved;
        }
        let snapshot = self.queue.snapshot();
        Session {
            tracks: snapshot
                .tracks
                .into_iter()
                .map(|track| SessionTrack {
                    artwork_url: self.artwork_url(track.id),
                    track,
                })
                .collect(),
            current: snapshot.current,
            position: self.playback.position,
            volume: self.playback.volume,
            shuffle: snapshot.shuffle,
            repeat: snapshot.repeat,
        }
    }

    /// Saves for the last time, on the actor thread, and says so. Taking the
    /// store lock waits for a write in progress, and the sequence number makes
    /// any write still queued skip itself.
    pub(super) fn shutdown(&mut self) {
        if let Some(store) = self.store.clone().filter(|_| self.dirty) {
            let session = self.session();
            self.save_seq += 1;
            let mut store = store.lock().unwrap_or_else(PoisonError::into_inner);
            store.last_seq = self.save_seq;
            if let Err(error) = store::save_session(&mut store.conn, &session) {
                tracing::warn!(%error, "could not save the session on exit");
            }
        }
        self.save_settings_now();
        self.emit(Event::Stopped);
        self.stopped = true;
    }

    /// Writes the session off the actor loop. Newer writes win.
    pub(super) fn save_session(&mut self) {
        let Some(store) = self.store.clone() else {
            return;
        };
        self.last_save = Instant::now();
        self.dirty = true;
        let session = self.session();
        self.save_seq += 1;
        let seq = self.save_seq;
        tokio::task::spawn_blocking(move || {
            let mut store = store.lock().unwrap_or_else(PoisonError::into_inner);
            if seq <= store.last_seq {
                return;
            }
            store.last_seq = seq;
            if let Err(error) = store::save_session(&mut store.conn, &session) {
                tracing::warn!(%error, "could not save the session");
            }
        });
    }

    pub(super) fn record_history(&self, track: TrackSummary) {
        let Some(store) = self.store.clone() else {
            return;
        };
        let played_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        let artwork_url = self.artwork_url(track.id);
        tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(PoisonError::into_inner);
            if let Err(error) =
                store::record_play(&store.conn, &track, artwork_url.as_deref(), played_at)
            {
                tracing::warn!(%error, "could not record the history");
            }
        });
    }

    /// Brings back the saved queue, paused at the saved position. The stream
    /// is only resolved when the person presses play.
    pub(super) fn restore(&mut self, session: Session) {
        // Something already started: the person's choice wins.
        if self.current.is_some() || session.tracks.is_empty() {
            return;
        }
        let mut tracks = Vec::with_capacity(session.tracks.len());
        for item in session.tracks {
            if let Some(url) = item.artwork_url {
                self.restored_artwork.insert(item.track.id, url);
            }
            tracks.push(item.track);
        }
        self.queue
            .restore(tracks, session.current, session.shuffle, session.repeat);
        self.playback.volume = session.volume;
        self.to_audio(sc_audio::Command::SetVolume(session.volume));
        self.emit(Event::Queue(self.queue.snapshot()));

        // Covers already on disk show at once; only the current one may download.
        for id in self.queue.snapshot().tracks.iter().map(|t| t.id) {
            let Some(url) = self.artwork_url(id) else {
                continue;
            };
            let path = artwork::path_for(&self.artwork_dir, &url);
            if path.exists() && self.artwork_requested.insert(ArtKey::Track(id)) {
                self.emit(Event::Artwork {
                    key: ArtKey::Track(id),
                    path,
                });
            }
        }
        let Some(current) = self.queue.current_track().cloned() else {
            return;
        };
        self.current = Some(current.id);
        self.pending_restore = Some(session.position);
        self.playback.position = session.position;
        self.playback.duration = current.duration;
        let id = current.id;
        self.emit(Event::NowPlaying(current));
        // Not `set_state`: restoring is not a change worth saving, and writing
        // here could overwrite a newer session another instance just saved.
        self.playback.state = PlayState::Paused;
        self.emit(Event::Playback(self.playback));
        self.request_artwork(ArtKey::Track(id));
    }

    /// The database opened (or not): keep it and restore the saved session.
    pub(super) fn store_ready(&mut self, opened: Option<OpenedStore>) {
        let Some((store, session, reset)) = opened else {
            return;
        };
        self.store = Some(store);
        if self.settings_changed {
            self.save_settings();
        }
        if reset {
            self.emit(Event::Problem(Problem::StorageReset));
        }
        if let Some(session) = session {
            self.restore(session);
        }
    }

    /// Sets the volume now; the session is saved once the slider rests.
    pub(super) fn set_volume(&mut self, volume: f32) {
        let volume = volume.clamp(0.0, 1.0);
        self.to_audio(sc_audio::Command::SetVolume(volume));
        self.playback.volume = volume;
        self.dirty = true;
        self.emit(Event::Playback(self.playback));
        if let Some(timer) = self.volume_save.take() {
            timer.abort();
        }
        let inputs = self.inputs.clone();
        self.volume_save = Some(tokio::spawn(async move {
            tokio::time::sleep(VOLUME_SAVE_DELAY).await;
            let _ = inputs.send(Input::SaveDue);
        }));
    }
}
