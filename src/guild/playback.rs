use async_trait::async_trait;
use songbird::{
    events::EventData,
    input::Input,
    tracks::{PlayMode, Track, TrackHandle},
    Call, Event, EventContext, EventHandler, TrackEvent,
};
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;
use uuid::Uuid;

use super::seek::format_timestamp;
use super::state::GuildState;
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

        if matches!(track_state.playing, PlayMode::Stop) {
            return None;
        }

        if self.state.is_looping() {
            return None;
        }

        self.state.pop_front();
        self.state.reset_playback_flags();

        let state = self.state.clone();
        let call = Arc::clone(&self.call);
        tokio::spawn(async move {
            if let Err(e) = play_front(&call, &state).await {
                tracing::warn!(%e, "Nie udało się puścić następnego utworu");
                let _ = state
                    .notify_channel
                    .say(&state.discord_http, e)
                    .await;
            }
        });
        None
    }
}

fn input_from_audio(audio: &Arc<[u8]>) -> Input {
    Input::from(Arc::clone(audio))
}

async fn songbird_has_live_track(call: &Arc<Mutex<Call>>) -> bool {
    let handle = {
        let guard = call.lock().await;
        guard.queue().current()
    };
    let Some(handle) = handle else {
        return false;
    };
    match handle.get_info().await {
        Ok(info) => matches!(info.playing, PlayMode::Play | PlayMode::Pause),
        Err(_) => false,
    }
}

async fn start_buffered(
    call: &Arc<Mutex<Call>>,
    state: &GuildState,
    audio: Arc<[u8]>,
) -> Result<TrackHandle, String> {
    let track_id = Uuid::new_v4();
    state.set_front_track_id(track_id);
    state.set_front_audio(Arc::clone(&audio));

    let mut track = Track::new_with_uuid(input_from_audio(&audio), track_id);
    track.events.add_event(
        EventData::new(
            Event::Track(TrackEvent::End),
            PlaylistSync {
                state: state.clone(),
                call: Arc::clone(call),
            },
        ),
        Duration::ZERO,
    );

    let mut guard = call.lock().await;
    let queue = guard.queue().clone();
    queue.stop();
    let handle = queue.add_with_preload(track, &mut *guard, None);
    drop(guard);

    crate::voice::attach_track_error_logger(
        &handle,
        state.notify_channel,
        Arc::clone(&state.discord_http),
        Some(state.looping_flag()),
    );
    tracing::info!(
        title = state.front_entry().map(|e| e.title).unwrap_or_default(),
        bytes = audio.len(),
        "Start odtwarzania"
    );
    Ok(handle)
}

async fn play_front(
    call: &Arc<Mutex<Call>>,
    state: &GuildState,
) -> Result<(), String> {
    let _gate = state.play_gate().lock().await;
    if songbird_has_live_track(call).await {
        return Ok(());
    }

    loop {
        let Some(entry) = state.front_entry() else {
            return Ok(());
        };

        let expected_query = entry.query.clone();
        let audio = match expected_query
            .clone()
            .into_buffered_audio(state.http_client.clone())
            .await
        {
            Ok(audio) => audio,
            Err(e) => {
                tracing::warn!(title = %entry.title, error = %e, "Pomijam utwór — pobieranie nieudane");
                let _ = state
                    .notify_channel
                    .say(
                        &state.discord_http,
                        format!("Nie udało się pobrać **{}**: {e}", entry.title),
                    )
                    .await;
                if state.front_entry().is_some_and(|front| front.query == expected_query) {
                    state.pop_front();
                }
                continue;
            }
        };

        let Some(front) = state.front_entry() else {
            return Ok(());
        };
        if front.query != expected_query {
            return Ok(());
        }

        start_buffered(call, state, audio).await?;
        return Ok(());
    }
}

/// Dodaje utwór: od razu gra, albo tylko do kolejki (bez pobierania audio).
pub async fn enqueue(
    call: &Arc<Mutex<Call>>,
    state: &GuildState,
    title: String,
    duration: Option<Duration>,
    query: YoutubeQuery,
) -> Result<usize, String> {
    let already_playing = songbird_has_live_track(call).await;
    state.push_entry(title, Uuid::nil(), duration, query, None);
    let position = state.len().saturating_sub(1);

    if !already_playing {
        play_front(call, state).await?;
    }

    Ok(position)
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
    if state.len() == 0 {
        return Err("Kolejka jest pusta.".to_string());
    }

    let title = state
        .front_entry()
        .map(|e| e.title)
        .unwrap_or_else(|| "nieznany utwór".to_string());

    // Zdejmij bieżący utwór z playlisty zanim Songbird wyśle End/Stop —
    // inaczej PlaylistSync mógłby zdjąć też następny.
    state.pop_front();
    state.reset_playback_flags();

    {
        let guard = call.lock().await;
        let queue = guard.queue();
        if let Some(current) = queue.current() {
            let _ = current.stop();
        }
        queue.stop();
    }

    if let Err(e) = play_front(call, state).await {
        return Err(format!("Pominięto: **{title}** — {e}"));
    }

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

    let handle = current_handle(call).await?;

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
