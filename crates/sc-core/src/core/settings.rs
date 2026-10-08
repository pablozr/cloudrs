//! The person's settings (ADR 0017): kept in memory, echoed to the UI and
//! saved off the actor loop.

use std::sync::PoisonError;

use sc_api::SoundCloudApi;

use super::Core;
use crate::store;
use crate::{Event, Settings};

impl<A: SoundCloudApi + 'static> Core<A> {
    /// Keeps and saves the settings, then tells the UI what is now in effect.
    pub(super) fn set_settings(&mut self, settings: Settings) {
        if settings != self.settings {
            self.settings = settings;
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
}
