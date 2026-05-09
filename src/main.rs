use serenity::{
    async_trait,
    prelude::*,
};
use serenity::model::{
    channel::Message,
    gateway::Ready,
};
use songbird::SerenityInit;

struct Handler;

#[async_trait]
impl EventHandler for Handler {

    async fn message(&self, ctx: Context, new_message: Message) {

        match new_message.content.as_str() {

            "!join" => {

                let guild_id = match new_message.guild_id {
                    Some(id) => id,

                    None => {
                        let _ = new_message
                            .reply(
                                &ctx.http,
                                "Ta komenda działa tylko na serwerze."
                            )
                            .await;

                        return;
                    }
                };

                let channel_id = ctx.cache.guild(guild_id)
                    .and_then(|guild| {
                        guild
                            .voice_states
                            .get(&new_message.author.id)
                            .and_then(|vs| vs.channel_id)
                    });

                let connect_to = match channel_id {

                    Some(channel) => channel,

                    None => {
                        let _ = new_message
                            .reply(
                                &ctx.http,
                                "Najpierw wejdź na kanał."
                            )
                            .await;

                        return;
                    }
                };

                let manager = songbird::get(&ctx)
                    .await
                    .expect("Songbird Voice client")
                    .clone();

                let _ = manager
                    .join(guild_id, connect_to)
                    .await;

            }

            _ => {}
        }
    }
}
#[tokio::main]
async fn main() {
    let token_env = format!("{}/src/discord_token.env", env!("CARGO_MANIFEST_DIR"));
    let _ = dotenvy::from_filename(token_env).or_else(|_| dotenvy::dotenv());

    let token = std::env::var("TOKEN").expect(
        "Brak zmiennej środowiskowej TOKEN — ustaw ją lub uzupełnij src/discord_token.env",
    );

    let intents =
        GatewayIntents::GUILDS
            | GatewayIntents::GUILD_MESSAGES
            | GatewayIntents::MESSAGE_CONTENT
            | GatewayIntents::GUILD_VOICE_STATES;

    let mut client = Client::builder(token, intents)
        .event_handler(Handler)
        .register_songbird()
        .await
        .expect("Błąd tworzenia klienta");

    if let Err(why) = client.start().await {
        println!("Client error: {:?}", why);
    }
}