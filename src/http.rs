use reqwest::Client;
use serenity::prelude::TypeMapKey;

pub struct HttpKey;

impl TypeMapKey for HttpKey {
    type Value = Client;
}
