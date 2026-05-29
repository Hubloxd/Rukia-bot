use dashmap::DashMap;
use parking_lot::Mutex;
use serenity::model::id::{ChannelId, GuildId};
use serenity::prelude::TypeMapKey;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct PlaylistEntry {
    pub title: String,
    pub track_id: Uuid,
    pub duration: Option<Duration>,
}

#[derive(Clone)]
pub struct GuildState {
    pub notify_channel: ChannelId,
    playlist: Arc<Mutex<VecDeque<PlaylistEntry>>>,
}

impl GuildState {
    pub fn new(notify_channel: ChannelId) -> Self {
        Self {
            notify_channel,
            playlist: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    pub fn push_entry(&self, title: String, track_id: Uuid, duration: Option<Duration>) {
        self.playlist.lock().push_back(PlaylistEntry {
            title,
            track_id,
            duration,
        });
    }

    pub fn front_duration(&self) -> Option<Duration> {
        self.playlist.lock().front().and_then(|e| e.duration)
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
    }

    pub fn playlist_arc(&self) -> Arc<Mutex<VecDeque<PlaylistEntry>>> {
        Arc::clone(&self.playlist)
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

    pub fn get_or_create(&self, guild_id: GuildId, notify_channel: ChannelId) -> GuildState {
        self.inner
            .entry(guild_id)
            .or_insert_with(|| GuildState::new(notify_channel))
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
