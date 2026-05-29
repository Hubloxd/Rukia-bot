use async_trait::async_trait;
use serenity::{
    model::channel::Message,
    model::id::{ChannelId, GuildId},
    prelude::Context,
};
use songbird::{
    tracks::PlayMode,
    Call, Event, EventContext, EventHandler, TrackEvent,
};
use std::sync::Arc;
use tokio::sync::Mutex;

pub async fn join_user_channel(
    ctx: &Context,
    msg: &Message,
    guild_id: GuildId,
) -> Result<Arc<Mutex<Call>>, String> {
    let channel_id = ctx.cache.guild(guild_id).and_then(|guild| {
        guild
            .voice_states
            .get(&msg.author.id)
            .and_then(|vs| vs.channel_id)
    });

    let connect_to = channel_id.ok_or_else(|| "Najpierw wejdź na kanał głosowy.".to_string())?;

    let manager = songbird::get(ctx)
        .await
        .ok_or_else(|| "Songbird nie jest zarejestrowany.".to_string())?;

    manager
        .join(guild_id, connect_to)
        .await
        .map_err(|e| format!("Nie udało się dołączyć do kanału: {e}"))?;

    tracing::info!(%guild_id, %connect_to, "Połączono z kanałem głosowym");
    Ok(manager.get(guild_id).expect("handler po join"))
}

pub async fn ensure_call(
    ctx: &Context,
    msg: &Message,
    guild_id: GuildId,
) -> Result<Arc<Mutex<Call>>, String> {
    let manager = songbird::get(ctx)
        .await
        .ok_or_else(|| "Songbird nie jest zarejestrowany.".to_string())?;

    if let Some(call) = manager.get(guild_id) {
        return Ok(call);
    }

    join_user_channel(ctx, msg, guild_id).await
}

struct TrackErrorLogger {
    channel_id: ChannelId,
    http: Arc<serenity::http::Http>,
}

#[async_trait]
impl EventHandler for TrackErrorLogger {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        let EventContext::Track(states) = ctx else {
            return None;
        };

        let (state, _) = states.first()?;
        let PlayMode::Errored(err) = &state.playing else {
            return None;
        };

        tracing::error!(?err, "Błąd odtwarzania utworu");

        let text = if crate::guild::is_seek_past_end(&err) {
            "Nie można przewinąć poza długość utworu.".to_string()
        } else {
            format!("Błąd odtwarzania: {err}")
        };

        let _ = self.channel_id.say(&self.http, text).await;

        Some(Event::Cancel)
    }
}

pub fn attach_track_error_logger(
    handle: &songbird::tracks::TrackHandle,
    channel_id: ChannelId,
    http: Arc<serenity::http::Http>,
) {
    let logger = TrackErrorLogger { channel_id, http };

    if let Err(e) = handle.add_event(Event::Track(TrackEvent::Error), logger) {
        tracing::warn!(?e, "Nie udało się podpiąć logera błędów tracku");
    }
}
