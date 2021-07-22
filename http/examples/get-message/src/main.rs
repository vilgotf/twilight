use futures::future;
use std::{env, error::Error, num::NonZeroU64};
use twilight_http::Client;
use twilight_model::id::ChannelId;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    // Initialize the tracing subscriber.
    tracing_subscriber::fmt::init();

    let client = Client::new(env::var("DISCORD_TOKEN")?);
    let channel_id = ChannelId(NonZeroU64::new(381_926_291_785_383_946).expect("non zero"));

    future::join_all((1u8..=10).map(|x| {
        client
            .create_message(channel_id)
            .content(&format!("Ping #{}", x))
            .expect("content not a valid length")
            .exec()
    }))
    .await;

    let me = client.current_user().exec().await?.model().await?;
    println!("Current user: {}#{}", me.name, me.discriminator);

    Ok(())
}
