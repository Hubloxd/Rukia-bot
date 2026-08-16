use dashmap::DashMap;
use parking_lot::Mutex;
use reqwest::Client;
use serenity::http::Http;
use serenity::model::id::{ChannelId, GuildId};
use serenity::prelude::TypeMapKey;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
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
}

#[derive(Clone)]
pub struct GuildState {
    pub notify_channel: ChannelId,
    pub http_client: Client,
    pub discord_http: Arc<Http>,
    playlist: Arc<Mutex<VecDeque<PlaylistEntry>>>,
    paused: Arc<AtomicBool>,
    looping: Arc<AtomicBool>,
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
        }
    }

    pub fn push_entry(
        &self,
        title: String,
        track_id: Uuid,
        duration: Option<Duration>,
        query: YoutubeQuery,
    ) {
        self.playlist.lock().push_back(PlaylistEntry {
            title,
            track_id,
            duration,
            query,
        });
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
        self.playlist.lock().pop_front()
    }

    pub fn len(&self) -> usize {
        self.playlist.lock().len()
    }

    pub fn entries_snapshot(&self) -> Vec<PlaylistEntry> {
        self.playlist.lock().iter().cloned().collect()
    }

    pub fn clear(&self) {
        self.playlist.lock().clear();
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
    }

    pub fn reset_playback_flags(&self) {
        self.set_paused(false);
        self.set_looping(false);
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
