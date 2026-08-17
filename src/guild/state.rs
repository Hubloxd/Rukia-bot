use dashmap::DashMap;
use parking_lot::Mutex;
use reqwest::Client;
use serenity::http::Http;
use serenity::model::id::{ChannelId, GuildId};
use serenity::prelude::TypeMapKey;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

use crate::youtube::YoutubeQuery;

#[derive(Clone, Debug)]
pub struct PlaylistEntry {
    pub title: String,
    pub track_id: Uuid,
    pub duration: Option<Duration>,
    pub query: YoutubeQuery,
    pub audio: Option<Arc<[u8]>>,
}

#[derive(Clone)]
pub struct GuildState {
    pub notify_channel: ChannelId,
    pub http_client: Client,
    pub discord_http: Arc<Http>,
    playlist: Arc<Mutex<VecDeque<PlaylistEntry>>>,
    paused: Arc<AtomicBool>,
    looping: Arc<AtomicBool>,
    loop_fail_streak: Arc<AtomicU8>,
    current_audio: Arc<Mutex<Option<Arc<[u8]>>>>,
    play_gate: Arc<tokio::sync::Mutex<()>>,
}

impl GuildState {
    pub fn new(notify_channel: ChannelId, http_client: Client, discord_http: Arc<Http>) -> Self {
        Self {
            notify_channel,
            http_client,
            discord_http,
            playlist: Arc::new(Mutex::new(VecDeque::new())),
            paused: Arc::new(AtomicBool::new(false)),
            looping: Arc::new(AtomicBool::new(false)),
            loop_fail_streak: Arc::new(AtomicU8::new(0)),
            current_audio: Arc::new(Mutex::new(None)),
            play_gate: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub fn push_entry(
        &self,
        title: String,
        track_id: Uuid,
        duration: Option<Duration>,
        query: YoutubeQuery,
        audio: Option<Arc<[u8]>>,
    ) {
        self.playlist.lock().push_back(PlaylistEntry {
            title,
            track_id,
            duration,
            query,
            audio,
        });
    }

    pub fn set_front_audio(&self, audio: Arc<[u8]>) {
        if let Some(front) = self.playlist.lock().front_mut() {
            front.audio = Some(Arc::clone(&audio));
        }
        self.set_current_audio(Some(audio));
    }

    pub fn front_duration(&self) -> Option<Duration> {
        self.playlist.lock().front().and_then(|e| e.duration)
    }

    pub fn front_entry(&self) -> Option<PlaylistEntry> {
        self.playlist.lock().front().cloned()
    }

    pub fn set_front_track_id(&self, track_id: Uuid) {
        if let Some(front) = self.playlist.lock().front_mut() {
            front.track_id = track_id;
        }
    }

    pub fn pop_front(&self) -> Option<PlaylistEntry> {
        let mut list = self.playlist.lock();
        let gone = list.pop_front();
        let next = list.front().and_then(|e| e.audio.clone());
        drop(list);
        self.set_current_audio(next);
        gone
    }

    pub fn set_current_audio(&self, audio: Option<Arc<[u8]>>) {
        *self.current_audio.lock() = audio;
    }

    pub fn current_audio(&self) -> Option<Arc<[u8]>> {
        self.current_audio.lock().clone()
    }

    pub fn len(&self) -> usize {
        self.playlist.lock().len()
    }

    pub fn entries_snapshot(&self) -> Vec<PlaylistEntry> {
        self.playlist.lock().iter().cloned().collect()
    }

    pub fn clear(&self) {
        self.playlist.lock().clear();
        self.set_current_audio(None);
        self.set_paused(false);
        self.set_looping(false);
    }

    pub fn playlist_arc(&self) -> Arc<Mutex<VecDeque<PlaylistEntry>>> {
        Arc::clone(&self.playlist)
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    pub fn set_paused(&self, value: bool) {
        self.paused.store(value, Ordering::SeqCst);
    }

    pub fn is_looping(&self) -> bool {
        self.looping.load(Ordering::SeqCst)
    }

    pub fn set_looping(&self, value: bool) {
        self.looping.store(value, Ordering::SeqCst);
        if !value {
            self.reset_loop_fail_streak();
        }
    }

    pub fn looping_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.looping)
    }

    pub fn bump_loop_fail_streak(&self) -> u8 {
        self.loop_fail_streak
            .fetch_add(1, Ordering::SeqCst)
            .saturating_add(1)
    }

    pub fn reset_loop_fail_streak(&self) {
        self.loop_fail_streak.store(0, Ordering::SeqCst);
    }

    pub fn reset_playback_flags(&self) {
        self.set_paused(false);
        self.set_looping(false);
        self.reset_loop_fail_streak();
    }

    pub fn play_gate(&self) -> &Arc<tokio::sync::Mutex<()>> {
        &self.play_gate
    }
}

#[derive(Clone, Default)]
pub struct GuildStates {
    inner: Arc<DashMap<GuildId, GuildState>>,
}

impl GuildStates {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DashMap::new()),
        }
    }

    pub fn get_or_create(
        &self,
        guild_id: GuildId,
        notify_channel: ChannelId,
        http_client: Client,
        discord_http: Arc<Http>,
    ) -> GuildState {
        self.inner
            .entry(guild_id)
            .or_insert_with(|| GuildState::new(notify_channel, http_client, discord_http))
            .value()
            .clone()
    }

    pub fn get(&self, guild_id: GuildId) -> Option<GuildState> {
        self.inner.get(&guild_id).map(|e| e.value().clone())
    }

    pub fn remove(&self, guild_id: GuildId) {
        self.inner.remove(&guild_id);
    }
}

pub struct GuildStatesKey;

impl TypeMapKey for GuildStatesKey {
    type Value = GuildStates;
}
