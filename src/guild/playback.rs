use async_trait::async_trait;
use songbird::{
    input::Input,
    tracks::{PlayMode, Track, TrackHandle},
    Call, Event, EventContext, EventHandler, TrackEvent,
};
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;
use uuid::Uuid;

use super::seek::format_timestamp;
use super::state::{GuildState, PlaylistEntry};
use crate::youtube::YoutubeQuery;

struct PlaylistSync {
    state: GuildState,
    call: Arc<Mutex<Call>>,
}

#[async_trait]
impl EventHandler for PlaylistSync {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        let EventContext::Track(states) = ctx else {
            return None;
        };

        let (track_state, handle) = states.first()?;
        let ended_uuid = handle.uuid();

        let Some(front) = self.state.front_entry() else {
            return None;
        };
        if front.track_id != ended_uuid {
            return None;
        }

        let natural_end = matches!(track_state.playing, PlayMode::End);
        if natural_end && self.state.is_looping() {
            let state = self.state.clone();
            let call = Arc::clone(&self.call);
            tokio::spawn(async move {
                restart_looped_track(call, state, front, ended_uuid).await;
            });
            return None;
        }

        self.state.pop_front();
        self.state.reset_playback_flags();
        None
    }
}

async fn restart_looped_track(
    call: Arc<Mutex<Call>>,
    state: GuildState,
    entry: PlaylistEntry,
    ended_uuid: Uuid,
) {
    let input = entry.query.clone().into_input(state.http_client.clone());

    let mut guard = call.lock().await;
    let queue = guard.queue().clone();

    let displaced = queue.modify_queue(|q| q.drain(..).collect::<Vec<_>>());
    for track in &displaced {
        if track.uuid() == ended_uuid {
            let _ = track.stop();
        } else {
            let _ = track.pause();
        }
    }

    let preload = entry
        .duration
        .map(|d| d.saturating_sub(Duration::from_secs(5)));
    let handle = queue.add_with_preload(Track::from(input), &mut *guard, preload);

    queue.modify_queue(|q| {
        for track in displaced {
            if track.uuid() != ended_uuid {
                q.push_back(track);
            }
        }
    });
    drop(guard);

    state.set_front_track_id(handle.uuid());
    state.set_paused(false);
    attach_playlist_sync(&handle, &state, &call);
    crate::voice::attach_track_error_logger(
        &handle,
        state.notify_channel,
        Arc::clone(&state.discord_http),
    );

    tracing::info!(title = %entry.title, "Ponowne odtworzenie utworu (pętla)");
}

fn attach_playlist_sync(handle: &TrackHandle, state: &GuildState, call: &Arc<Mutex<Call>>) {
    let _ = handle.add_event(
        Event::Track(TrackEvent::End),
        PlaylistSync {
            state: state.clone(),
            call: Arc::clone(call),
        },
    );
}

pub async fn enqueue(
    call: &Arc<Mutex<Call>>,
    state: &GuildState,
    input: Input,
    title: String,
    duration: Option<Duration>,
    query: YoutubeQuery,
) -> Result<(TrackHandle, usize), String> {
    let mut guard = call.lock().await;
    let queue = guard.queue().clone();
    let position = queue.len();

    let preload = duration.map(|d| d.saturating_sub(Duration::from_secs(5)));
    let handle = queue.add_with_preload(Track::from(input), &mut *guard, preload);
    drop(guard);

    state.push_entry(title, handle.uuid(), duration, query);
    attach_playlist_sync(&handle, state, call);

    Ok((handle, position))
}

pub fn format_queue_list(state: &GuildState) -> String {
    let entries = state.entries_snapshot();
    if entries.is_empty() {
        return "Kolejka jest pusta.".to_string();
    }

    let mut lines: Vec<String> = Vec::new();

    let mut flags = Vec::new();
    if state.is_paused() {
        flags.push("⏸ pauza");
    }
    if state.is_looping() {
        flags.push("🔁 pętla");
    }
    if !flags.is_empty() {
        lines.push(flags.join(" · "));
    }

    lines.extend(entries.iter().enumerate().map(|(i, e)| {
        let marker = if i == 0 { "▶ " } else { "  " };
        format!("{marker}{}. {}", i + 1, e.title)
    }));

    lines.join("\n")
}

pub async fn skip_current(
    call: &Arc<Mutex<Call>>,
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
    state.reset_playback_flags();

    Ok(format!("Pominięto: **{title}**"))
}

pub fn clear_playlist(state: &GuildState) {
    state.clear();
}

fn seek_error_message(err: impl std::fmt::Display) -> String {
    let msg = err.to_string();
    if msg.contains("end of stream") {
        return "Nie można przewinąć poza długość utworu.".to_string();
    }
    format!("Nie udało się przewinąć: {msg}")
}

pub async fn seek_current(
    call: &Arc<Mutex<Call>>,
    state: &GuildState,
    target: Duration,
) -> Result<String, String> {
    if let Some(duration) = state.front_duration() {
        if target > duration {
            return Err(format!(
                "Nie można przewinąć poza długość utworu (maks. **{}**).",
                format_timestamp(duration)
            ));
        }
    }

    let handle = {
        let guard = call.lock().await;
        let queue = guard.queue();
        if queue.is_empty() {
            return Err("Kolejka jest pusta — użyj `!play`.".to_string());
        }
        queue
            .current()
            .ok_or_else(|| "Brak aktywnego utworu.".to_string())?
    };

    match handle.seek_async(target).await {
        Ok(position) => Ok(format!(
            "Przewinięto do **{}**.",
            format_timestamp(position)
        )),
        Err(e) => Err(seek_error_message(e)),
    }
}

async fn current_handle(call: &Arc<Mutex<Call>>) -> Result<TrackHandle, String> {
    let guard = call.lock().await;
    let queue = guard.queue();
    if queue.is_empty() {
        return Err("Kolejka jest pusta — użyj `!play`.".to_string());
    }
    queue
        .current()
        .ok_or_else(|| "Brak aktywnego utworu.".to_string())
}

pub async fn toggle_pause(
    call: &Arc<Mutex<Call>>,
    state: &GuildState,
) -> Result<String, String> {
    let handle = current_handle(call).await?;

    if state.is_paused() {
        handle
            .play()
            .map_err(|e| format!("Nie udało się wznowić: {e}"))?;
        state.set_paused(false);
        Ok("Wznowiono.".to_string())
    } else {
        handle
            .pause()
            .map_err(|e| format!("Nie udało się wstrzymać: {e}"))?;
        state.set_paused(true);
        Ok("Wstrzymano.".to_string())
    }
}

pub async fn toggle_loop(
    call: &Arc<Mutex<Call>>,
    state: &GuildState,
) -> Result<String, String> {
    let _handle = current_handle(call).await?;

    if state.is_looping() {
        state.set_looping(false);
        Ok("Pętla wyłączona.".to_string())
    } else {
        state.set_looping(true);
        Ok("Pętla włączona — aktualny utwór będzie odtwarzany w kółko.".to_string())
    }
}
