use futures::future;
use std::{error::Error, num::NonZeroU64};
use twilight_http::Client;
use twilight_model::id::ChannelId;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    // Initialize the tracing subscriber.
    tracing_subscriber::fmt::init();

    let client = Client::builder()
        .proxy("localhost:3000".to_owned(), true)
        .ratelimiter(None)
        .build();
    let channel_id = ChannelId(NonZeroU64::new(620_980_184_606_048_278).expect("non zero"));

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
