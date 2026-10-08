//! Playing: the queue's current track, streams, skipping, autoplay and the
//! audio engine's events (ADR 0004, ADR 0007).

use std::sync::Arc;
use std::time::Duration;

use sc_api::models::Track;
use sc_api::{SoundCloudApi, StreamProtocol, StreamSource};

use super::{Core, Input};
use crate::queue::Step;
use crate::types::{ArtKey, ListId, PlayState, Problem, TrackId, TrackSummary};
use crate::{Event, artwork, waveform};

/// How often the session is saved while playing.
const SAVE_EVERY: Duration = Duration::from_secs(5);
/// Tracks fetched when the queue runs out.
const AUTOPLAY_COUNT: u32 = 20;
/// How long before the end of a track the next one is prepared (ADR 0022).
const PRELOAD_BEFORE: Duration = Duration::from_secs(20);

/// The next track being prepared, so playback can move on without a gap.
#[derive(Debug)]
pub(super) struct Preload {
    /// The play it belongs to; a newer play makes it stale.
    generation: u64,
    /// The queue entry it was prepared for.
    key: u64,
    /// The audio engine has been told to open it.
    sent: bool,
}

impl<A: SoundCloudApi + 'static> Core<A> {
    /// Seeks the audio, or moves the saved position of a restored track that
    /// has no stream yet.
    pub(super) fn seek(&mut self, at: Duration) {
        if self.pending_restore.is_some() {
            self.pending_restore = Some(at);
            self.playback.position = at;
            self.emit(Event::Playback(self.playback));
        } else {
            self.to_audio(sc_audio::Command::Seek(at));
        }
    }

    /// A click in a list: the queue becomes the rows loaded so far, starting here.
    pub(super) fn play_from_list(&mut self, list: ListId, id: TrackId) {
        let context = self.lists.get(&list).map(|state| &state.tracks);
        if let Some(context) = context
            && let Some(start) = context.iter().position(|t| t.id == id)
        {
            self.queue.set_context(context.clone(), start);
        } else if let Some(summary) = self.find_summary(id) {
            self.queue.set_context(vec![summary], 0);
        } else {
            return;
        }
        self.queue_changed();
        self.play_current(None, false);
    }

    /// Queues a track seen in any list. With nothing playing it starts.
    pub(super) fn enqueue(&mut self, id: TrackId, next: bool) {
        let Some(summary) = self.find_summary(id) else {
            return;
        };
        if self.queue.current_track().is_none() {
            self.queue.set_context(vec![summary], 0);
            self.queue_changed();
            self.play_current(None, false);
            return;
        }
        if next {
            self.queue.play_next(summary);
        } else {
            self.queue.add_to_queue(summary);
        }
        self.queue_changed();
        // A Jam guest's track may never have been on a list here.
        self.request_artwork(ArtKey::Track(id));
    }

    pub(super) fn queue_changed(&mut self) {
        self.emit(Event::Queue(self.queue.snapshot()));
        self.host_broadcast_queue();
        self.save_session();
        // The track after this one may have changed too.
        let stale = self
            .preload
            .as_ref()
            .is_some_and(|p| self.queue.peek_next().map(|(key, _)| key) != Some(p.key));
        if stale {
            self.cancel_preload();
        }
    }

    /// Where the artwork of a track comes from: this session's tracks, or a
    /// restored queue.
    pub(super) fn artwork_url(&self, id: TrackId) -> Option<String> {
        self.tracks
            .get(&id)
            .and_then(|t| t.artwork(artwork::SIZE))
            .or_else(|| self.restored_artwork.get(&id).cloned())
    }

    /// The next track, or related tracks when the queue is over.
    pub(super) fn skip_forward(&mut self, ended: bool) {
        match self.queue.next(ended) {
            Step::Play(_) => {
                self.queue_changed();
                self.play_current(None, true);
            }
            Step::End => self.autoplay(),
        }
    }

    pub(super) fn autoplay(&mut self) {
        let Some(last) = self.queue.last_track().map(|t| t.id) else {
            return;
        };
        if self.autoplay.is_some() {
            return;
        }
        self.autoplay_gen += 1;
        let generation = self.autoplay_gen;
        self.autoplay = Some(generation);
        let (api, inputs) = (Arc::clone(&self.api), self.inputs.clone());
        tokio::spawn(async move {
            let result = api.related(last.0, AUTOPLAY_COUNT).await;
            let _ = inputs.send(Input::RelatedDone { generation, result });
        });
    }

    pub(super) fn related_done(
        &mut self,
        generation: u64,
        result: sc_api::Result<sc_api::models::Page<Track>>,
    ) {
        if self.autoplay != Some(generation) {
            return;
        }
        self.autoplay = None;
        let page = match result {
            Ok(page) => page,
            Err(error) => {
                self.emit(Event::Problem(Problem::from_api(&error)));
                return;
            }
        };
        let summaries: Vec<TrackSummary> =
            page.collection.iter().map(TrackSummary::from_api).collect();
        for track in page.collection {
            self.tracks.insert(TrackId(track.id), track);
        }
        let ids: Vec<TrackId> = summaries.iter().map(|t| t.id).collect();
        let added = self.queue.extend_context(summaries);
        if added == 0 {
            return;
        }
        self.queue_changed();
        for id in ids {
            self.request_artwork(ArtKey::Track(id));
        }
        // Moving on started this fetch (the track ended or Next was pressed on
        // the last one), and any newer play would have cancelled it.
        self.skip_forward(true);
    }

    /// The current track cannot be fetched or streamed. Tell the person, and
    /// move on unless they picked this track or a full pass already failed.
    pub(super) fn play_failed(&mut self, problem: Problem) {
        self.set_state(PlayState::Idle);
        // A Jam guest tells the host and waits for its next track.
        let in_jam = self.jam_play_failed(&problem);
        self.emit(Event::Problem(problem));
        if in_jam {
            return;
        }
        self.failed_in_row += 1;
        if self.skip_on_failure && self.failed_in_row < self.queue.len() {
            self.skip_forward(false);
        }
    }

    /// Plays the track at the queue's current position.
    /// `moving_on`: reached by Next, a track end or a restore, so a failure
    /// skips ahead.
    pub(super) fn play_current(&mut self, start_at: Option<Duration>, moving_on: bool) {
        let Some(summary) = self.queue.current_track().cloned() else {
            return;
        };
        self.play_summary(summary, start_at, moving_on);
    }

    /// Plays this track (the queue's current one, or a Jam guest's).
    pub(super) fn play_summary(
        &mut self,
        summary: TrackSummary,
        start_at: Option<Duration>,
        moving_on: bool,
    ) {
        let id = summary.id;
        self.begin_track(summary, start_at, moving_on, PlayState::Loading);
        let generation = self.play_gen;
        match self.tracks.get(&id).cloned() {
            Some(track) => self.start_stream(track, start_at),
            None => {
                let (api, inputs) = (Arc::clone(&self.api), self.inputs.clone());
                tokio::spawn(async move {
                    let result = api.track(id.0).await.map(Box::new);
                    let _ = inputs.send(Input::TrackFetched {
                        track: id,
                        generation,
                        start_at,
                        result,
                    });
                });
            }
        }
    }

    /// Everything that happens when a track becomes the current one, except
    /// getting its stream: `state` is Loading, or Playing when the audio
    /// engine already moved on by itself.
    fn begin_track(
        &mut self,
        summary: TrackSummary,
        start_at: Option<Duration>,
        moving_on: bool,
        state: PlayState,
    ) {
        let id = summary.id;
        self.cancel_preload();
        self.current = Some(id);
        self.autoplay = None;
        self.play_gen += 1;
        self.jam_track_starts(id, start_at);
        self.skip_on_failure = moving_on;
        if !moving_on {
            self.failed_in_row = 0;
        }
        self.pending_restore = None;
        self.listened.reset();
        self.playback.position = start_at.unwrap_or_default();
        self.playback.duration = summary.duration;
        self.emit(Event::NowPlaying(summary));
        self.set_state(state);
        self.request_artwork(ArtKey::Track(id));
        self.save_session();
    }

    pub(super) fn start_stream(&mut self, track: Track, start_at: Option<Duration>) {
        self.track_extras(&track);
        let generation = self.play_gen;
        let (api, inputs) = (Arc::clone(&self.api), self.inputs.clone());
        tokio::spawn(async move {
            let result = api.stream_url(&track).await;
            let _ = inputs.send(Input::StreamReady {
                generation,
                start_at,
                result,
            });
        });
    }

    /// What the current track shows besides its audio: its links and waveform.
    fn track_extras(&mut self, track: &Track) {
        let id = TrackId(track.id);
        self.emit(Event::NowPlayingLinks {
            track: id,
            cover_url: track.artwork("t500x500"),
            page_url: Some(track.permalink_url.clone()).filter(|url| !url.is_empty()),
        });
        let Some(url) = track.waveform_url.clone() else {
            return;
        };
        let generation = self.play_gen;
        let (api, inputs) = (Arc::clone(&self.api), self.inputs.clone());
        tokio::spawn(async move {
            if let Ok(wave) = api.waveform(&url).await {
                let bars = waveform::to_bars(&wave.samples, wave.height, waveform::BARS);
                let _ = inputs.send(Input::WaveformReady {
                    track: id,
                    generation,
                    bars,
                });
            }
        });
    }

    /// Prepares the track that follows the current one, once the current one
    /// is close to its end. It acts at most once per play.
    fn maybe_preload(&mut self, position: Duration) {
        if self.preload.is_some()
            || self.jam.is_some()
            || self.playback.state != PlayState::Playing
            || self.playback.duration.is_zero()
            || position + PRELOAD_BEFORE < self.playback.duration
        {
            return;
        }
        let Some((key, next)) = self.queue.peek_next() else {
            return;
        };
        // A preview is not the whole track; it takes the normal path.
        if next.preview_only {
            return;
        }
        let id = next.id;
        let cached = self.tracks.get(&id).cloned();
        let generation = self.play_gen;
        self.preload = Some(Preload {
            generation,
            key,
            sent: false,
        });
        let (api, inputs) = (Arc::clone(&self.api), self.inputs.clone());
        tokio::spawn(async move {
            let (track, result) = match cached {
                Some(track) => {
                    let result = api.stream_url(&track).await;
                    (None, result)
                }
                None => match api.track(id.0).await {
                    Ok(track) => {
                        let result = api.stream_url(&track).await;
                        (Some(Box::new(track)), result)
                    }
                    Err(error) => (None, Err(error)),
                },
            };
            let _ = inputs.send(Input::PreloadReady {
                generation,
                key,
                track,
                result,
            });
        });
    }

    pub(super) fn preload_ready(
        &mut self,
        generation: u64,
        key: u64,
        track: Option<Box<Track>>,
        result: sc_api::Result<StreamSource>,
    ) {
        let current = self
            .preload
            .as_ref()
            .is_some_and(|p| p.generation == generation && p.key == key);
        if !current {
            return;
        }
        if let Some(track) = track {
            self.tracks.insert(TrackId(track.id), *track);
        }
        match result {
            Ok(stream) => {
                self.to_audio(sc_audio::Command::Preload(to_source(stream)));
                if let Some(p) = self.preload.as_mut() {
                    p.sent = true;
                }
            }
            // No retry: the track takes the normal path when this one ends.
            Err(error) => tracing::debug!(%error, "could not prepare the next track"),
        }
    }

    /// Forgets the prepared track, telling the engine if it was handed over.
    pub(super) fn cancel_preload(&mut self) {
        if let Some(p) = self.preload.take()
            && p.sent
        {
            self.to_audio(sc_audio::Command::CancelPreload);
        }
    }

    /// The engine started the preloaded track by itself: move the queue and
    /// the screens on to it without loading anything.
    fn next_started(&mut self) {
        let Some(p) = self.preload.take() else {
            // The engine played a preload that was cancelled in time: the
            // normal path loads whatever is next now.
            return self.skip_forward(true);
        };
        if self.jam.is_some() {
            return self.skip_forward(true);
        }
        match self.queue.next(true) {
            Step::Play(_) if self.queue.current_key() == Some(p.key) => {
                self.queue_changed();
                let Some(summary) = self.queue.current_track().cloned() else {
                    return;
                };
                let id = summary.id;
                self.begin_track(summary, None, true, PlayState::Playing);
                self.failed_in_row = 0;
                if let Some(track) = self.tracks.get(&id).cloned() {
                    self.track_extras(&track);
                }
            }
            // The queue changed under the preload: play what is next now.
            Step::Play(_) => {
                self.queue_changed();
                self.play_current(None, true);
            }
            Step::End => self.autoplay(),
        }
    }

    pub(super) fn set_state(&mut self, state: PlayState) {
        self.playback.state = state;
        self.emit(Event::Playback(self.playback));
        if state == PlayState::Paused {
            self.save_session();
        }
    }

    pub(super) fn audio_event(&mut self, event: sc_audio::Event) {
        match event {
            sc_audio::Event::State(state) => {
                let state = match state {
                    sc_audio::PlaybackState::Idle => PlayState::Idle,
                    sc_audio::PlaybackState::Loading => PlayState::Loading,
                    sc_audio::PlaybackState::Playing => PlayState::Playing,
                    sc_audio::PlaybackState::Paused => PlayState::Paused,
                    sc_audio::PlaybackState::Ended => PlayState::Ended,
                };
                self.set_state(state);
                // A long pause could outlive the prepared stream's URL; the
                // next position while playing prepares it again.
                if state == PlayState::Paused {
                    self.cancel_preload();
                }
                self.jam_state_changed(state);
                if state == PlayState::Ended && !self.jam_owns_track_end() {
                    self.skip_forward(true);
                }
            }
            sc_audio::Event::Position(position) => {
                self.playback.position = position;
                self.emit(Event::Playback(self.playback));
                self.maybe_preload(position);
                self.jam_position(position);
                if self.listened.tick(position)
                    && let Some(track) = self.queue.current_track().cloned()
                {
                    self.record_history(track);
                }
                if self.last_save.elapsed() >= SAVE_EVERY {
                    self.save_session();
                }
            }
            sc_audio::Event::NextStarted => self.next_started(),
            sc_audio::Event::DeviceLost => self.output_fell_back(Problem::OutputDeviceLost),
            sc_audio::Event::DeviceMissing => self.output_fell_back(Problem::OutputDeviceMissing),
            sc_audio::Event::Error(detail) => {
                tracing::warn!(%detail, "audio error");
                self.emit(Event::Problem(Problem::Audio(detail)));
            }
        }
    }

    /// A queued track that was not cached (a restored queue) arrived.
    pub(super) fn track_fetched(
        &mut self,
        track: TrackId,
        start_at: Option<Duration>,
        result: sc_api::Result<Box<Track>>,
    ) {
        match result {
            Ok(api_track) => {
                self.tracks.insert(track, (*api_track).clone());
                self.start_stream(*api_track, start_at);
            }
            Err(error) => self.play_failed(Problem::from_api(&error)),
        }
    }

    /// The current track's stream URL: hand it to the audio engine.
    pub(super) fn stream_ready(
        &mut self,
        start_at: Option<Duration>,
        result: sc_api::Result<StreamSource>,
    ) {
        let stream = match result {
            Ok(stream) => stream,
            Err(error) => return self.play_failed(Problem::from_api(&error)),
        };
        self.failed_in_row = 0;
        let source = to_source(stream);
        if self.jam_prepare(&source, start_at) {
            return;
        }
        let _ = self.audio.send(sc_audio::Command::Load(source));
        if let Some(at) = start_at {
            let _ = self.audio.send(sc_audio::Command::Seek(at));
        }
    }
}

fn to_source(stream: StreamSource) -> sc_audio::Source {
    let kind = match stream.protocol {
        StreamProtocol::Hls => sc_audio::SourceKind::Hls,
        StreamProtocol::Progressive => sc_audio::SourceKind::Progressive,
    };
    sc_audio::Source {
        url: stream.url,
        kind,
    }
}
