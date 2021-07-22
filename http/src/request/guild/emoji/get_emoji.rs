use crate::{client::Client, request::Request, response::ResponseFuture, routing::Route};
use twilight_model::{
    guild::Emoji,
    id::{EmojiId, GuildId},
};

/// Get an emoji for a guild by the the guild's ID and emoji's ID.
///
/// # Examples
///
/// Get emoji `100` from guild `50`:
///
/// ```no_run
/// use twilight_http::Client;
/// use twilight_model::id::{EmojiId, GuildId};
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::new("my token".to_owned());
///
/// let guild_id = GuildId::new(50).expect("non zero");
/// let emoji_id = EmojiId::new(100).expect("non zero");
///
/// client.emoji(guild_id, emoji_id).exec().await?;
/// # Ok(()) }
/// ```
pub struct GetEmoji<'a> {
    emoji_id: EmojiId,
    guild_id: GuildId,
    http: &'a Client,
}

impl<'a> GetEmoji<'a> {
    pub(crate) const fn new(http: &'a Client, guild_id: GuildId, emoji_id: EmojiId) -> Self {
        Self {
            emoji_id,
            guild_id,
            http,
        }
    }

    /// Execute the request, returning a future resolving to a [`Response`].
    ///
    /// [`Response`]: crate::response::Response
    pub fn exec(self) -> ResponseFuture<Emoji> {
        let request = Request::from_route(Route::GetEmoji {
            emoji_id: self.emoji_id.get(),
            guild_id: self.guild_id.get(),
        });

        self.http.request(request)
    }
}
