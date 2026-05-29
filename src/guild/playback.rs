use async_trait::async_trait;
use parking_lot::Mutex;
use songbird::{
    input::Input,
    tracks::TrackHandle,
    Call, Event, EventContext, EventHandler, TrackEvent,
};
use std::{collections::VecDeque, sync::Arc};
use super::state::{GuildState, PlaylistEntry};

struct PlaylistSync {
    playlist: Arc<Mutex<VecDeque<PlaylistEntry>>>,
}

#[async_trait]
impl EventHandler for PlaylistSync {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        let EventContext::Track(states) = ctx else {
            return None;
        };

        let (_, handle) = states.first()?;
        let mut list = self.playlist.lock();

        if list
            .front()
            .is_some_and(|e| e.track_id == handle.uuid())
        {
            list.pop_front();
        }

        None
    }
}

fn attach_playlist_sync(handle: &TrackHandle, playlist: Arc<Mutex<VecDeque<PlaylistEntry>>>) {
    let _ = handle.add_event(
        Event::Track(TrackEvent::End),
        PlaylistSync { playlist },
    );
}

pub async fn enqueue(
    call: &Arc<tokio::sync::Mutex<Call>>,
    state: &GuildState,
    input: Input,
    title: String,
) -> Result<(TrackHandle, usize), String> {
    let mut guard = call.lock().await;
    let queue = guard.queue().clone();
    let position = queue.len();

    let handle = queue.add_source(input, &mut *guard).await;
    state.push_entry(title, handle.uuid());
    attach_playlist_sync(&handle, state.playlist_arc());

    Ok((handle, position))
}

pub fn format_queue_list(state: &GuildState) -> String {
    let entries = state.entries_snapshot();
    if entries.is_empty() {
        return "Kolejka jest pusta.".to_string();
    }

    entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let marker = if i == 0 { "▶ " } else { "  " };
            format!("{marker}{}. {}", i + 1, e.title)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub async fn skip_current(
    call: &Arc<tokio::sync::Mutex<Call>>,
    state: &GuildState,
) -> Result<String, String> {
    let guard = call.lock().await;
    let queue = guard.queue();

    if queue.is_empty() {
        return Err("Kolejka jest pusta.".to_string());
    }

    let skipped = queue
        .dequeue(0)
        .ok_or_else(|| "Brak utworu do pominięcia.".to_string())?;

    let title = state
        .pop_front()
        .map(|e| e.title)
        .unwrap_or_else(|| "nieznany utwór".to_string());

    let _ = skipped.stop();
    let _ = queue.resume();

    Ok(format!("Pominięto: **{title}**"))
}

pub fn clear_playlist(state: &GuildState) {
    state.clear();
}
