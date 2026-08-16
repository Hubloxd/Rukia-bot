use async_trait::async_trait;
use songbird::{
    input::Input,
    tracks::TrackHandle,
    Call, Event, EventContext, EventHandler, TrackEvent,
};
use std::{sync::Arc, time::Duration};

use super::seek::format_timestamp;
use super::state::GuildState;

struct PlaylistSync {
    state: GuildState,
}

#[async_trait]
impl EventHandler for PlaylistSync {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        let EventContext::Track(states) = ctx else {
            return None;
        };

        let (_, handle) = states.first()?;
        let playlist = self.state.playlist_arc();
        let mut list = playlist.lock();

        if list
            .front()
            .is_some_and(|e| e.track_id == handle.uuid())
        {
            list.pop_front();
        }
        drop(list);

        self.state.reset_playback_flags();

        None
    }
}

fn attach_playlist_sync(handle: &TrackHandle, state: &GuildState) {
    let _ = handle.add_event(
        Event::Track(TrackEvent::End),
        PlaylistSync {
            state: state.clone(),
        },
    );
}

pub async fn enqueue(
    call: &Arc<tokio::sync::Mutex<Call>>,
    state: &GuildState,
    input: Input,
    title: String,
    duration: Option<Duration>,
) -> Result<(TrackHandle, usize), String> {
    let mut guard = call.lock().await;
    let queue = guard.queue().clone();
    let position = queue.len();

    let handle = queue.add_source(input, &mut *guard).await;
    state.push_entry(title, handle.uuid(), duration);
    attach_playlist_sync(&handle, state);

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
    call: &Arc<tokio::sync::Mutex<Call>>,
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

async fn current_handle(
    call: &Arc<tokio::sync::Mutex<Call>>,
) -> Result<TrackHandle, String> {
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
    call: &Arc<tokio::sync::Mutex<Call>>,
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
    call: &Arc<tokio::sync::Mutex<Call>>,
    state: &GuildState,
) -> Result<String, String> {
    let handle = current_handle(call).await?;

    if state.is_looping() {
        handle
            .disable_loop()
            .map_err(|e| format!("Nie udało się wyłączyć pętli: {e}"))?;
        state.set_looping(false);
        Ok("Pętla wyłączona.".to_string())
    } else {
        handle
            .enable_loop()
            .map_err(|e| format!("Nie udało się włączyć pętli: {e}"))?;
        state.set_looping(true);
        Ok("Pętla włączona — aktualny utwór będzie odtwarzany w kółko.".to_string())
    }
}
