//! The person's settings (ADR 0017): kept in memory, echoed to the UI and
//! saved off the actor loop.

use std::collections::HashSet;
use std::sync::PoisonError;

use sc_api::SoundCloudApi;

use super::{Core, Input};
use crate::types::{ArtKey, Problem};
use crate::{Event, Settings, artwork, store};

impl<A: SoundCloudApi + 'static> Core<A> {
    /// Keeps and saves the settings, then tells the UI what is now in effect.
    pub(super) fn set_settings(&mut self, settings: Settings) {
        if settings != self.settings {
            let old = std::mem::replace(&mut self.settings, settings.clone());
            if settings.output_device != old.output_device {
                self.to_audio(sc_audio::Command::SetDevice(settings.output_device));
            }
            if settings.normalize != old.normalize {
                self.to_audio(sc_audio::Command::SetNormalize(settings.normalize));
            }
            if settings.equalizer != old.equalizer {
                self.to_audio(sc_audio::Command::SetEqualizer(settings.equalizer.gains()));
            }
            if settings.volume_boost != old.volume_boost {
                self.to_audio(sc_audio::Command::SetVolumeBoost(settings.volume_boost));
                // Above 100% only exists with the boost.
                if !settings.volume_boost && self.playback.volume > 1.0 {
                    self.set_volume(1.0);
                }
            }
            self.settings_changed = true;
            self.save_settings();
        }
        self.emit(Event::Settings(self.settings.clone()));
    }

    /// Writes the settings off the actor loop. Newer writes win. Without a
    /// store yet, `store_ready` writes them once it opens.
    pub(super) fn save_settings(&mut self) {
        let Some(store) = self.store.clone() else {
            return;
        };
        self.settings_seq += 1;
        let seq = self.settings_seq;
        let settings = self.settings.clone();
        tokio::task::spawn_blocking(move || {
            let mut store = store.lock().unwrap_or_else(PoisonError::into_inner);
            if seq <= store.settings_seq {
                return;
            }
            store.settings_seq = seq;
            if let Err(error) = store::save_settings(&store.conn, &settings) {
                tracing::warn!(%error, "could not save the settings");
            }
        });
    }

    /// The last write of the settings, on the actor thread. Taking the store
    /// lock waits for a write in progress, and the sequence number makes any
    /// write still queued skip itself.
    pub(super) fn save_settings_now(&mut self) {
        let Some(store) = self.store.clone().filter(|_| self.settings_changed) else {
            return;
        };
        self.settings_seq += 1;
        let mut store = store.lock().unwrap_or_else(PoisonError::into_inner);
        store.settings_seq = self.settings_seq;
        if let Err(error) = store::save_settings(&store.conn, &self.settings) {
            tracing::warn!(%error, "could not save the settings on exit");
        }
    }

    /// The player left the chosen device: tell the person and go back to the
    /// system default, so the next start does not try the missing one again.
    pub(super) fn output_fell_back(&mut self, problem: Problem) {
        self.emit(Event::Problem(problem));
        if self.settings.output_device.is_some() {
            self.settings.output_device = None;
            self.settings_changed = true;
            self.save_settings();
            self.emit(Event::Settings(self.settings.clone()));
        }
    }

    /// Lists the audio devices off the actor loop (WASAPI opens each one).
    pub(super) fn list_output_devices(&self) {
        let inputs = self.inputs.clone();
        tokio::task::spawn_blocking(move || {
            let _ = inputs.send(Input::OutputDevices(sc_audio::output_devices()));
        });
    }

    /// Measures the artwork cache off the actor loop.
    pub(super) fn measure_cache(&self) {
        let (dir, inputs) = (self.artwork_dir.clone(), self.inputs.clone());
        tokio::task::spawn_blocking(move || {
            let _ = inputs.send(Input::CacheMeasured(artwork::disk_usage(&dir)));
        });
    }

    /// Deletes the covers this session is not showing, so nothing on screen
    /// points at a file that is gone.
    pub(super) fn clear_cache(&self) {
        let in_use: HashSet<_> = self
            .artwork_requested
            .iter()
            .filter_map(|key| match key {
                ArtKey::Track(id) => self.artwork_url(*id),
                ArtKey::User(_) | ArtKey::Playlist(_) => self.other_art.get(key).cloned(),
            })
            .flat_map(|url| {
                let path = artwork::path_for(&self.artwork_dir, &url);
                // A download in flight writes the partial file first.
                [path.with_extension("part"), path]
            })
            .collect();
        let (dir, inputs) = (self.artwork_dir.clone(), self.inputs.clone());
        tokio::task::spawn_blocking(move || {
            let (remaining, complete) = artwork::clear_except(&dir, &in_use);
            let _ = inputs.send(Input::CacheCleared {
                remaining,
                complete,
            });
        });
    }

    pub(super) fn cache_cleared(&self, remaining: u64, complete: bool) {
        if complete {
            self.emit(Event::CacheCleared);
        } else {
            self.emit(Event::Problem(Problem::CacheNotCleared));
        }
        self.emit(Event::CacheSize(remaining));
    }
}
