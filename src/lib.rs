pub mod guild;
pub mod http;
pub mod voice;
pub mod youtube;

use guild::{GuildStates, GuildStatesKey};
use http::HttpKey;
use serenity::{
    async_trait,
    model::channel::Message,
    prelude::*,
};
use songbird::SerenityInit;
use youtube::YoutubeError;

struct Handler;

async fn guild_states(ctx: &Context) -> GuildStates {
    let data = ctx.data.read().await;
    data.get::<GuildStatesKey>()
        .cloned()
        .expect("GuildStates missing from TypeMap")
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, msg: Message) {
        if let Some(arg) = msg.content.strip_prefix("!play ") {
            handle_play(&ctx, &msg, arg).await;
            return;
        }

        if let Some(arg) = msg.content.strip_prefix("!seek ") {
            handle_seek(&ctx, &msg, arg).await;
            return;
        }

        match msg.content.as_str() {
            "!play" => {
                let _ = msg
                    .reply(&ctx.http, "Użycie: `!play <url lub wyszukiwanie>`")
                    .await;
            }
            "!join" => handle_join(&ctx, &msg).await,
            "!queue" => handle_queue(&ctx, &msg).await,
            "!skip" => handle_skip(&ctx, &msg).await,
            "!pause" => handle_pause(&ctx, &msg).await,
            "!loop" => handle_loop(&ctx, &msg).await,
            "!seek" => {
                let _ = msg
                    .reply(
                        &ctx.http,
                        "Użycie: `!seek <czas>` — np. `45`, `1:30`, `1:05:20`",
                    )
                    .await;
            }
            "!leave" => handle_leave(&ctx, &msg).await,
            _ => {}
        }
    }
}

async fn handle_join(ctx: &Context, msg: &Message) {
    let guild_id = match require_guild(msg) {
        Ok(id) => id,
        Err(reply) => {
            let _ = msg.reply(&ctx.http, reply).await;
            return;
        }
    };

    let _ = guild_states(ctx).await.get_or_create(guild_id, msg.channel_id);

    match voice::join_user_channel(ctx, msg, guild_id).await {
        Ok(_) => {
            let _ = msg.reply(&ctx.http, "Dołączono do kanału.").await;
        }
        Err(e) => {
            let _ = msg.reply(&ctx.http, e).await;
        }
    }
}

async fn handle_play(ctx: &Context, msg: &Message, arg: &str) {
    let guild_id = match require_guild(msg) {
        Ok(id) => id,
        Err(reply) => {
            let _ = msg.reply(&ctx.http, reply).await;
            return;
        }
    };

    let query = match youtube::YoutubeQuery::parse(arg) {
        Ok(q) => q,
        Err(YoutubeError::InvalidInput) => {
            let _ = msg
                .reply(&ctx.http, "Użycie: `!play <url lub wyszukiwanie>`")
                .await;
            return;
        }
        Err(e) => {
            let _ = msg.reply(&ctx.http, e.to_string()).await;
            return;
        }
    };

    let http_client = {
        let data = ctx.data.read().await;
        data.get::<HttpKey>()
            .cloned()
            .expect("HTTP client missing from TypeMap")
    };

    let states = guild_states(ctx).await;
    let guild_state = states.get_or_create(guild_id, msg.channel_id);

    let call = match voice::ensure_call(ctx, msg, guild_id).await {
        Ok(c) => c,
        Err(e) => {
            let _ = msg.reply(&ctx.http, e).await;
            return;
        }
    };

    let track_info = match query.resolve_metadata(&http_client).await {
        Ok(info) => info,
        Err(e) => {
            let _ = msg.reply(&ctx.http, e.to_string()).await;
            return;
        }
    };

    let title = track_info
        .title
        .clone()
        .unwrap_or_else(|| query.text().to_string());
    let duration = track_info.duration.map(std::time::Duration::from_secs);

    let input = query.into_input(http_client);

    let (track_handle, position) =
        match guild::enqueue(&call, &guild_state, input, title.clone(), duration).await
    {
        Ok(v) => v,
        Err(e) => {
            let _ = msg.reply(&ctx.http, e).await;
            return;
        }
    };

    voice::attach_track_error_logger(
        &track_handle,
        guild_state.notify_channel,
        ctx.http.clone(),
    );

    let reply = if position == 0 {
        format!("Odtwarzam: **{title}**")
    } else {
        format!("Dodano do kolejki (#{}): **{title}**", position + 1)
    };
    let _ = msg.reply(&ctx.http, reply).await;
}

async fn handle_queue(ctx: &Context, msg: &Message) {
    let guild_id = match require_guild(msg) {
        Ok(id) => id,
        Err(reply) => {
            let _ = msg.reply(&ctx.http, reply).await;
            return;
        }
    };

    let states = guild_states(ctx).await;
    let Some(state) = states.get(guild_id) else {
        let _ = msg
            .reply(&ctx.http, "Brak kolejki na tym serwerze. Użyj `!play`.")
            .await;
        return;
    };

    let _ = msg
        .reply(&ctx.http, guild::format_queue_list(&state))
        .await;
}

async fn handle_seek(ctx: &Context, msg: &Message, arg: &str) {
    let guild_id = match require_guild(msg) {
        Ok(id) => id,
        Err(reply) => {
            let _ = msg.reply(&ctx.http, reply).await;
            return;
        }
    };

    let target = match guild::parse_seek_position(arg) {
        Ok(d) => d,
        Err(e) => {
            let _ = msg.reply(&ctx.http, e).await;
            return;
        }
    };

    let states = guild_states(ctx).await;
    let Some(state) = states.get(guild_id) else {
        let _ = msg.reply(&ctx.http, "Nic nie gra — użyj `!play`.").await;
        return;
    };

    let manager = match songbird::get(ctx).await {
        Some(m) => m,
        None => {
            let _ = msg.reply(&ctx.http, "Bot nie jest na kanale głosowym.").await;
            return;
        }
    };

    let Some(call) = manager.get(guild_id) else {
        let _ = msg.reply(&ctx.http, "Bot nie jest na kanale głosowym.").await;
        return;
    };

    match guild::seek_current(&call, &state, target).await {
        Ok(text) => {
            let _ = msg.reply(&ctx.http, text).await;
        }
        Err(e) => {
            let _ = msg.reply(&ctx.http, e).await;
        }
    }
}

async fn handle_skip(ctx: &Context, msg: &Message) {
    let guild_id = match require_guild(msg) {
        Ok(id) => id,
        Err(reply) => {
            let _ = msg.reply(&ctx.http, reply).await;
            return;
        }
    };

    let states = guild_states(ctx).await;
    let Some(state) = states.get(guild_id) else {
        let _ = msg.reply(&ctx.http, "Nic nie gra — użyj `!play`.").await;
        return;
    };

    let manager = match songbird::get(ctx).await {
        Some(m) => m,
        None => {
            let _ = msg.reply(&ctx.http, "Bot nie jest na kanale głosowym.").await;
            return;
        }
    };

    let Some(call) = manager.get(guild_id) else {
        let _ = msg.reply(&ctx.http, "Bot nie jest na kanale głosowym.").await;
        return;
    };

    match guild::skip_current(&call, &state).await {
        Ok(text) => {
            let _ = msg.reply(&ctx.http, text).await;
        }
        Err(e) => {
            let _ = msg.reply(&ctx.http, e).await;
        }
    }
}

async fn handle_pause(ctx: &Context, msg: &Message) {
    let guild_id = match require_guild(msg) {
        Ok(id) => id,
        Err(reply) => {
            let _ = msg.reply(&ctx.http, reply).await;
            return;
        }
    };

    let states = guild_states(ctx).await;
    let Some(state) = states.get(guild_id) else {
        let _ = msg.reply(&ctx.http, "Nic nie gra — użyj `!play`.").await;
        return;
    };

    let manager = match songbird::get(ctx).await {
        Some(m) => m,
        None => {
            let _ = msg.reply(&ctx.http, "Bot nie jest na kanale głosowym.").await;
            return;
        }
    };

    let Some(call) = manager.get(guild_id) else {
        let _ = msg.reply(&ctx.http, "Bot nie jest na kanale głosowym.").await;
        return;
    };

    match guild::toggle_pause(&call, &state).await {
        Ok(text) => {
            let _ = msg.reply(&ctx.http, text).await;
        }
        Err(e) => {
            let _ = msg.reply(&ctx.http, e).await;
        }
    }
}

async fn handle_loop(ctx: &Context, msg: &Message) {
    let guild_id = match require_guild(msg) {
        Ok(id) => id,
        Err(reply) => {
            let _ = msg.reply(&ctx.http, reply).await;
            return;
        }
    };

    let states = guild_states(ctx).await;
    let Some(state) = states.get(guild_id) else {
        let _ = msg.reply(&ctx.http, "Nic nie gra — użyj `!play`.").await;
        return;
    };

    let manager = match songbird::get(ctx).await {
        Some(m) => m,
        None => {
            let _ = msg.reply(&ctx.http, "Bot nie jest na kanale głosowym.").await;
            return;
        }
    };

    let Some(call) = manager.get(guild_id) else {
        let _ = msg.reply(&ctx.http, "Bot nie jest na kanale głosowym.").await;
        return;
    };

    match guild::toggle_loop(&call, &state).await {
        Ok(text) => {
            let _ = msg.reply(&ctx.http, text).await;
        }
        Err(e) => {
            let _ = msg.reply(&ctx.http, e).await;
        }
    }
}

async fn handle_leave(ctx: &Context, msg: &Message) {
    let guild_id = match require_guild(msg) {
        Ok(id) => id,
        Err(reply) => {
            let _ = msg.reply(&ctx.http, reply).await;
            return;
        }
    };

    let states = guild_states(ctx).await;
    if let Some(state) = states.get(guild_id) {
        guild::clear_playlist(&state);
    }
    states.remove(guild_id);

    let manager = match songbird::get(ctx).await {
        Some(m) => m,
        None => {
            let _ = msg.reply(&ctx.http, "Bot nie był na kanale głosowym.").await;
            return;
        }
    };

    match manager.remove(guild_id).await {
        Ok(()) => {
            let _ = msg.reply(&ctx.http, "Opuszczono kanał i wyczyszczono kolejkę.").await;
        }
        Err(e) => {
            let _ = msg
                .reply(&ctx.http, format!("Nie udało się opuścić kanału: {e}"))
                .await;
        }
    }
}

fn require_guild(msg: &Message) -> Result<serenity::model::id::GuildId, &'static str> {
    msg.guild_id
        .ok_or("Ta komenda działa tylko na serwerze.")
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,songbird=debug".into()),
        )
        .try_init();
}

pub async fn run() {
    init_tracing();

    if std::env::var("TOKEN").is_err() && std::env::var("DISCORD_TOKEN").is_err() {
        let token_env = format!("{}/src/discord_token.env", env!("CARGO_MANIFEST_DIR"));
        let _ = dotenvy::from_filename(&token_env).or_else(|_| dotenvy::dotenv());
    }

    let token = std::env::var("TOKEN").or_else(|_| std::env::var("DISCORD_TOKEN")).expect(
        "Brak TOKEN lub DISCORD_TOKEN — ustaw zmienną środowiskową lub plik src/discord_token.env",
    );

    let intents = GatewayIntents::GUILDS
        | GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT
        | GatewayIntents::GUILD_VOICE_STATES;

    let mut client = Client::builder(token, intents)
        .event_handler(Handler)
        .register_songbird()
        .await
        .expect("Błąd tworzenia klienta");

    {
        let mut data = client.data.write().await;
        data.insert::<HttpKey>(reqwest::Client::new());
        data.insert::<GuildStatesKey>(GuildStates::new());
    }

    tracing::info!("Bot uruchomiony (stan per gildia)");

    if let Err(why) = client.start().await {
        tracing::error!(?why, "Client error");
    }
}
