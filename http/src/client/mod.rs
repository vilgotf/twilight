mod builder;

pub use self::builder::ClientBuilder;

use crate::{
    error::{Error, ErrorType},
    ratelimiting::Ratelimiter,
    request::{
        application::{
            CreateFollowupMessage, CreateGlobalCommand, CreateGuildCommand, DeleteFollowupMessage,
            DeleteGlobalCommand, DeleteGuildCommand, DeleteOriginalResponse, GetCommandPermissions,
            GetGlobalCommands, GetGuildCommandPermissions, GetGuildCommands, InteractionCallback,
            InteractionError, InteractionErrorType, SetCommandPermissions, SetGlobalCommands,
            SetGuildCommands, UpdateCommandPermissions, UpdateFollowupMessage, UpdateGlobalCommand,
            UpdateGuildCommand, UpdateOriginalResponse,
        },
        channel::{
            reaction::delete_reaction::TargetUser,
            stage::create_stage_instance::CreateStageInstanceError,
        },
        guild::{
            create_guild::CreateGuildError, create_guild_channel::CreateGuildChannelError,
            update_guild_channel_positions::Position,
        },
        prelude::*,
        GetUserApplicationInfo, Method, Request,
    },
    response::ResponseFuture,
    API_VERSION,
};
use hyper::{
    client::{Client as HyperClient, HttpConnector},
    header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, USER_AGENT},
    Body,
};
use std::{
    convert::TryFrom,
    fmt::{Debug, Formatter, Result as FmtResult},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::time;
use twilight_model::{
    application::{
        callback::InteractionResponse,
        command::{permissions::CommandPermissions, Command},
    },
    channel::message::allowed_mentions::AllowedMentions,
    guild::Permissions,
    id::{
        ApplicationId, ChannelId, CommandId, EmojiId, GuildId, IntegrationId, InteractionId,
        MessageId, RoleId, UserId, WebhookId,
    },
};

#[cfg(feature = "hyper-rustls")]
type HttpsConnector<T> = hyper_rustls::HttpsConnector<T>;
#[cfg(all(feature = "hyper-tls", not(feature = "hyper-rustls")))]
type HttpsConnector<T> = hyper_tls::HttpsConnector<T>;

struct State {
    http: HyperClient<HttpsConnector<HttpConnector>, Body>,
    default_headers: Option<HeaderMap>,
    proxy: Option<Box<str>>,
    ratelimiter: Option<Ratelimiter>,
    timeout: Duration,
    token_invalid: Arc<AtomicBool>,
    token: Option<Box<str>>,
    use_http: bool,
    pub(crate) application_id: AtomicU64,
    pub(crate) default_allowed_mentions: Option<AllowedMentions>,
}

impl Debug for State {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.debug_struct("State")
            .field("http", &self.http)
            .field("default_headers", &self.default_headers)
            .field("proxy", &self.proxy)
            .field("ratelimiter", &self.ratelimiter)
            .field("token", &self.token)
            .field("use_http", &self.use_http)
            .finish()
    }
}

/// Twilight's http client.
///
/// Almost all of the client methods require authentication, and as such, the client must be
/// supplied with a Discord Token. Get yours [here].
///
/// # OAuth
///
/// To use Bearer tokens prefix the token with `"Bearer "`, including the space
/// at the end like so:
///
/// ```no_run
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// use std::env;
/// use twilight_http::Client;
///
/// let bearer = env::var("BEARER_TOKEN")?;
/// let token = format!("Bearer {}", bearer);
///
/// let client = Client::new(token);
/// # Ok(()) }
/// ```
///
/// # Cloning
///
/// The client internally wraps its data within an Arc. This means that the
/// client can be cloned and passed around tasks and threads cheaply.
///
/// # Unauthorized behavior
///
/// When the client encounters an Unauthorized response it will take note that
/// the configured token is invalid. This may occur when the token has been
/// revoked or expired. When this happens, you must create a new client with the
/// new token. The client will no longer execute requests in order to
/// prevent API bans and will always return [`ErrorType::Unauthorized`].
///
/// # Examples
///
/// Create a client called `client`:
/// ```rust,no_run
/// use twilight_http::Client;
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::new("my token".to_owned());
/// # Ok(()) }
/// ```
///
/// Use [`ClientBuilder`] to create a client called `client`, with a shorter
/// timeout:
/// ```rust,no_run
/// use twilight_http::Client;
/// use std::time::Duration;
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::builder()
///     .token("my token".to_owned())
///     .timeout(Duration::from_secs(5))
///     .build();
/// # Ok(()) }
/// ```
///
/// All the examples on this page assume you have already created a client, and have named it
/// `client`.
///
/// [here]: https://discord.com/developers/applications
#[derive(Clone, Debug)]
pub struct Client {
    state: Arc<State>,
}

impl Client {
    /// Create a new `hyper-rustls` or `hyper-tls` backed client with a token.
    #[cfg_attr(docsrs, doc(cfg(any(feature = "hyper-rustls", feature = "hyper-tls"))))]
    pub fn new(token: String) -> Self {
        ClientBuilder::default().token(token).build()
    }

    /// Create a new builder to create a client.
    ///
    /// Refer to its documentation for more information.
    pub fn builder() -> ClientBuilder {
        ClientBuilder::new()
    }

    /// Retrieve an immutable reference to the token used by the client.
    ///
    /// If the initial token provided is not prefixed with `Bot `, it will be, and this method
    /// reflects that.
    pub fn token(&self) -> Option<&str> {
        self.state.token.as_deref()
    }

    /// Retrieve the [`ApplicationId`] used by interaction methods.
    pub fn application_id(&self) -> Option<ApplicationId> {
        let id = self.state.application_id.load(Ordering::Relaxed);

        if id != 0 {
            return Some(ApplicationId::new(id).expect("non zero"));
        }

        None
    }

    /// Set a new [`ApplicationId`] after building the client.
    ///
    /// Returns the previous ID, if there was one.
    pub fn set_application_id(&self, application_id: ApplicationId) -> Option<ApplicationId> {
        let prev = self
            .state
            .application_id
            .swap(application_id.get(), Ordering::Relaxed);

        if prev != 0 {
            return Some(ApplicationId::new(prev).expect("non zero"));
        }

        None
    }

    /// Get the default [`AllowedMentions`] for sent messages.
    pub fn default_allowed_mentions(&self) -> Option<AllowedMentions> {
        self.state.default_allowed_mentions.clone()
    }

    /// Get the Ratelimiter used by the client internally.
    ///
    /// This will return `None` only if ratelimit handling
    /// has been explicitly disabled in the [`ClientBuilder`].
    pub fn ratelimiter(&self) -> Option<Ratelimiter> {
        self.state.ratelimiter.clone()
    }

    /// Get the audit log for a guild.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use twilight_http::Client;
    /// use twilight_model::id::GuildId;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("token".to_owned());
    /// let guild_id = GuildId::new(101).expect("non zero");
    /// let audit_log = client
    /// // not done
    ///     .audit_log(guild_id)
    ///     .exec()
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub const fn audit_log(&self, guild_id: GuildId) -> GetAuditLog<'_> {
        GetAuditLog::new(self, guild_id)
    }

    /// Retrieve the bans for a guild.
    ///
    /// # Examples
    ///
    /// Retrieve the bans for guild `1`:
    ///
    /// ```no_run
    /// # use twilight_http::Client;
    /// use twilight_model::id::GuildId;
    /// #
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("my token".to_owned());
    /// #
    /// let guild_id = GuildId::new(1).expect("non zero");
    ///
    /// let bans = client.bans(guild_id).exec().await?;
    /// # Ok(()) }
    /// ```
    pub const fn bans(&self, guild_id: GuildId) -> GetBans<'_> {
        GetBans::new(self, guild_id)
    }

    /// Get information about a ban of a guild.
    ///
    /// Includes the user banned and the reason.
    pub const fn ban(&self, guild_id: GuildId, user_id: UserId) -> GetBan<'_> {
        GetBan::new(self, guild_id, user_id)
    }

    /// Bans a user from a guild, optionally with the number of days' worth of
    /// messages to delete and the reason.
    ///
    /// # Examples
    ///
    /// Ban user `200` from guild `100`, deleting
    /// 1 day's worth of messages, for the reason `"memes"`:
    ///
    /// ```no_run
    /// # use twilight_http::{request::AuditLogReason, Client};
    /// use twilight_model::id::{GuildId, UserId};
    /// #
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("my token".to_owned());
    /// #
    /// let guild_id = GuildId::new(100).expect("non zero");
    /// let user_id = UserId::new(200).expect("non zero");
    /// client.create_ban(guild_id, user_id)
    ///     .delete_message_days(1)?
    ///     .reason("memes")?
    ///     .exec()
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub const fn create_ban(&self, guild_id: GuildId, user_id: UserId) -> CreateBan<'_> {
        CreateBan::new(self, guild_id, user_id)
    }

    /// Remove a ban from a user in a guild.
    ///
    /// # Examples
    ///
    /// Unban user `200` from guild `100`:
    ///
    /// ```no_run
    /// # use twilight_http::Client;
    /// use twilight_model::id::{GuildId, UserId};
    /// #
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("my token".to_owned());
    /// #
    /// let guild_id = GuildId::new(100).expect("non zero");
    /// let user_id = UserId::new(200).expect("non zero");
    ///
    /// client.delete_ban(guild_id, user_id).exec().await?;
    /// # Ok(()) }
    /// ```
    pub const fn delete_ban(&self, guild_id: GuildId, user_id: UserId) -> DeleteBan<'_> {
        DeleteBan::new(self, guild_id, user_id)
    }

    /// Get a channel by its ID.
    ///
    /// # Examples
    ///
    /// Get channel `100`:
    ///
    /// ```no_run
    /// # use twilight_http::Client;
    /// # use twilight_model::id::ChannelId;
    /// #
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("my token".to_owned());
    /// #
    /// let channel_id = ChannelId::new(100).expect("non zero");
    /// #
    /// let channel = client.channel(channel_id).exec().await?;
    /// # Ok(()) }
    /// ```
    pub const fn channel(&self, channel_id: ChannelId) -> GetChannel<'_> {
        GetChannel::new(self, channel_id)
    }

    /// Delete a channel by ID.
    pub const fn delete_channel(&self, channel_id: ChannelId) -> DeleteChannel<'_> {
        DeleteChannel::new(self, channel_id)
    }

    /// Update a channel.
    ///
    /// All fields are optional. The minimum length of the name is 2 UTF-16 characters and the
    /// maximum is 100 UTF-16 characters.
    pub const fn update_channel(&self, channel_id: ChannelId) -> UpdateChannel<'_> {
        UpdateChannel::new(self, channel_id)
    }

    /// Follows a news channel by [`ChannelId`].
    ///
    /// The type returned is [`FollowedChannel`].
    ///
    /// [`FollowedChannel`]: ::twilight_model::channel::FollowedChannel
    pub const fn follow_news_channel(
        &self,
        channel_id: ChannelId,
        webhook_channel_id: ChannelId,
    ) -> FollowNewsChannel<'_> {
        FollowNewsChannel::new(self, channel_id, webhook_channel_id)
    }

    /// Get the invites for a guild channel.
    ///
    /// Requires the [`MANAGE_CHANNELS`] permission. This method only works if
    /// the channel is of type [`GuildChannel`].
    ///
    /// [`MANAGE_CHANNELS`]: twilight_model::guild::Permissions::MANAGE_CHANNELS
    /// [`GuildChannel`]: twilight_model::channel::GuildChannel
    pub const fn channel_invites(&self, channel_id: ChannelId) -> GetChannelInvites<'_> {
        GetChannelInvites::new(self, channel_id)
    }

    /// Get channel messages, by [`ChannelId`].
    ///
    /// Only one of [`after`], [`around`], and [`before`] can be specified at a time.
    /// Once these are specified, the type returned is [`GetChannelMessagesConfigured`].
    ///
    /// If [`limit`] is unspecified, the default set by Discord is 50.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use twilight_http::Client;
    /// use twilight_model::id::{ChannelId, MessageId};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("my token".to_owned());
    /// let channel_id = ChannelId::new(123).expect("non zero");
    /// let message_id = MessageId::new(234).expect("non zero");
    /// let limit: u64 = 6;
    ///
    /// let messages = client
    ///     .channel_messages(channel_id)
    ///     .before(message_id)
    ///     .limit(limit)?
    ///     .exec()
    ///     .await?;
    ///
    /// # Ok(()) }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns a [`GetChannelMessagesErrorType::LimitInvalid`] error type if
    /// the amount is less than 1 or greater than 100.
    ///
    /// [`after`]: GetChannelMessages::after
    /// [`around`]: GetChannelMessages::around
    /// [`before`]: GetChannelMessages::before
    /// [`GetChannelMessagesConfigured`]: crate::request::channel::message::GetChannelMessagesConfigured
    /// [`limit`]: GetChannelMessages::limit
    /// [`GetChannelMessagesErrorType::LimitInvalid`]: crate::request::channel::message::get_channel_messages::GetChannelMessagesErrorType::LimitInvalid
    pub const fn channel_messages(&self, channel_id: ChannelId) -> GetChannelMessages<'_> {
        GetChannelMessages::new(self, channel_id)
    }

    pub const fn delete_channel_permission(
        &self,
        channel_id: ChannelId,
    ) -> DeleteChannelPermission<'_> {
        DeleteChannelPermission::new(self, channel_id)
    }

    /// Update the permissions for a role or a user in a channel.
    ///
    /// # Examples:
    ///
    /// Create permission overrides for a role to view the channel, but not send messages:
    ///
    /// ```no_run
    /// # use twilight_http::Client;
    /// use twilight_model::guild::Permissions;
    /// use twilight_model::id::{ChannelId, RoleId};
    /// #
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("my token".to_owned());
    ///
    /// let channel_id = ChannelId::new(123).expect("non zero");
    /// let allow = Permissions::VIEW_CHANNEL;
    /// let deny = Permissions::SEND_MESSAGES;
    /// let role_id = RoleId::new(432).expect("non zero");
    ///
    /// client.update_channel_permission(channel_id, allow, deny)
    ///     .role(role_id)
    ///     .exec()
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub const fn update_channel_permission(
        &self,
        channel_id: ChannelId,
        allow: Permissions,
        deny: Permissions,
    ) -> UpdateChannelPermission<'_> {
        UpdateChannelPermission::new(self, channel_id, allow, deny)
    }

    /// Get all the webhooks of a channel.
    pub const fn channel_webhooks(&self, channel_id: ChannelId) -> GetChannelWebhooks<'_> {
        GetChannelWebhooks::new(self, channel_id)
    }

    /// Get information about the current user.
    pub const fn current_user(&self) -> GetCurrentUser<'_> {
        GetCurrentUser::new(self)
    }

    /// Get information about the current bot application.
    pub const fn current_user_application(&self) -> GetUserApplicationInfo<'_> {
        GetUserApplicationInfo::new(self)
    }

    /// Update the current user.
    ///
    /// All parameters are optional. If the username is changed, it may cause the discriminator to
    /// be randomized.
    pub const fn update_current_user(&self) -> UpdateCurrentUser<'_> {
        UpdateCurrentUser::new(self)
    }

    /// Update the current user's voice state.
    ///
    /// All parameters are optional.
    ///
    /// # Caveats
    ///
    /// - `channel_id` must currently point to a stage channel.
    /// - Current user must have already joined `channel_id`.
    pub const fn update_current_user_voice_state(
        &self,
        guild_id: GuildId,
        channel_id: ChannelId,
    ) -> UpdateCurrentUserVoiceState<'_> {
        UpdateCurrentUserVoiceState::new(self, guild_id, channel_id)
    }

    /// Get the current user's connections.
    ///
    /// Requires the `connections` `OAuth2` scope.
    pub const fn current_user_connections(&self) -> GetCurrentUserConnections<'_> {
        GetCurrentUserConnections::new(self)
    }

    /// Returns a list of guilds for the current user.
    ///
    /// # Examples
    ///
    /// Get the first 25 guilds with an ID after `300` and before
    /// `400`:
    ///
    /// ```no_run
    /// # use twilight_http::Client;
    /// use twilight_model::id::GuildId;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("my token".to_owned());
    /// #
    /// let after = GuildId::new(300).expect("non zero");
    /// let before = GuildId::new(400).expect("non zero");
    /// let guilds = client.current_user_guilds()
    ///     .after(after)
    ///     .before(before)
    ///     .limit(25)?
    ///     .exec()
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub const fn current_user_guilds(&self) -> GetCurrentUserGuilds<'_> {
        GetCurrentUserGuilds::new(self)
    }

    /// Changes the user's nickname in a guild.
    pub const fn update_current_user_nick<'a>(
        &'a self,
        guild_id: GuildId,
        nick: &'a str,
    ) -> UpdateCurrentUserNick<'a> {
        UpdateCurrentUserNick::new(self, guild_id, nick)
    }

    /// Get the emojis for a guild, by the guild's id.
    ///
    /// # Examples
    ///
    /// Get the emojis for guild `100`:
    ///
    /// ```no_run
    /// # use twilight_http::Client;
    /// # use twilight_model::id::GuildId;
    /// #
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("my token".to_owned());
    /// #
    /// let guild_id = GuildId::new(100).expect("non zero");
    ///
    /// client.emojis(guild_id).exec().await?;
    /// # Ok(()) }
    /// ```
    pub const fn emojis(&self, guild_id: GuildId) -> GetEmojis<'_> {
        GetEmojis::new(self, guild_id)
    }

    /// Get an emoji for a guild by the the guild's ID and emoji's ID.
    ///
    /// # Examples
    ///
    /// Get emoji `100` from guild `50`:
    ///
    /// ```no_run
    /// # use twilight_http::Client;
    /// # use twilight_model::id::{EmojiId, GuildId};
    /// #
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("my token".to_owned());
    /// #
    /// let guild_id = GuildId::new(50).expect("non zero");
    /// let emoji_id = EmojiId::new(100).expect("non zero");
    ///
    /// client.emoji(guild_id, emoji_id).exec().await?;
    /// # Ok(()) }
    /// ```
    pub const fn emoji(&self, guild_id: GuildId, emoji_id: EmojiId) -> GetEmoji<'_> {
        GetEmoji::new(self, guild_id, emoji_id)
    }

    /// Create an emoji in a guild.
    ///
    /// The emoji must be a Data URI, in the form of `data:image/{type};base64,{data}` where
    /// `{type}` is the image MIME type and `{data}` is the base64-encoded image.  Refer to [the
    /// discord docs] for more information about image data.
    ///
    /// [the discord docs]: https://discord.com/developers/docs/reference#image-data
    pub const fn create_emoji<'a>(
        &'a self,
        guild_id: GuildId,
        name: &'a str,
        image: &'a str,
    ) -> CreateEmoji<'a> {
        CreateEmoji::new(self, guild_id, name, image)
    }

    /// Delete an emoji in a guild, by id.
    pub const fn delete_emoji(&self, guild_id: GuildId, emoji_id: EmojiId) -> DeleteEmoji<'_> {
        DeleteEmoji::new(self, guild_id, emoji_id)
    }

    /// Update an emoji in a guild, by id.
    pub const fn update_emoji(&self, guild_id: GuildId, emoji_id: EmojiId) -> UpdateEmoji<'_> {
        UpdateEmoji::new(self, guild_id, emoji_id)
    }

    /// Get information about the gateway, optionally with additional information detailing the
    /// number of shards to use and sessions remaining.
    ///
    /// # Examples
    ///
    /// Get the gateway connection URL without bot information:
    ///
    /// ```no_run
    /// # use twilight_http::Client;
    /// #
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("my token".to_owned());
    /// #
    /// let info = client.gateway().exec().await?;
    /// # Ok(()) }
    /// ```
    ///
    /// Get the gateway connection URL with additional shard and session information, which
    /// requires specifying a bot token:
    ///
    /// ```no_run
    /// # use twilight_http::Client;
    /// #
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("my token".to_owned());
    /// #
    /// let info = client.gateway().authed().exec().await?.model().await?;
    ///
    /// println!("URL: {}", info.url);
    /// println!("Recommended shards to use: {}", info.shards);
    /// # Ok(()) }
    /// ```
    pub const fn gateway(&self) -> GetGateway<'_> {
        GetGateway::new(self)
    }

    /// Get information about a guild.
    pub const fn guild(&self, guild_id: GuildId) -> GetGuild<'_> {
        GetGuild::new(self, guild_id)
    }

    /// Create a new request to create a guild.
    ///
    /// The minimum length of the name is 2 UTF-16 characters and the maximum is 100 UTF-16
    /// characters. This endpoint can only be used by bots in less than 10 guilds.
    ///
    /// # Errors
    ///
    /// Returns a [`CreateGuildErrorType::NameInvalid`] error type if the name
    /// length is too short or too long.
    ///
    /// [`CreateGuildErrorType::NameInvalid`]: crate::request::guild::create_guild::CreateGuildErrorType::NameInvalid
    pub fn create_guild(&self, name: String) -> Result<CreateGuild<'_>, CreateGuildError> {
        CreateGuild::new(self, name)
    }

    /// Delete a guild permanently. The user must be the owner.
    pub const fn delete_guild(&self, guild_id: GuildId) -> DeleteGuild<'_> {
        DeleteGuild::new(self, guild_id)
    }

    /// Update a guild.
    ///
    /// All endpoints are optional. Refer to [the discord docs] for more information.
    ///
    /// [the discord docs]: https://discord.com/developers/docs/resources/guild#modify-guild
    pub const fn update_guild(&self, guild_id: GuildId) -> UpdateGuild<'_> {
        UpdateGuild::new(self, guild_id)
    }

    /// Leave a guild by id.
    pub const fn leave_guild(&self, guild_id: GuildId) -> LeaveGuild<'_> {
        LeaveGuild::new(self, guild_id)
    }

    /// Get the channels in a guild.
    pub const fn guild_channels(&self, guild_id: GuildId) -> GetGuildChannels<'_> {
        GetGuildChannels::new(self, guild_id)
    }

    /// Create a new request to create a guild channel.
    ///
    /// All fields are optional except for name. The minimum length of the name is 2 UTF-16
    /// characters and the maximum is 100 UTF-16 characters.
    ///
    /// # Errors
    ///
    /// Returns a [`CreateGuildChannelErrorType::NameInvalid`] error type when
    /// the length of the name is either fewer than 2 UTF-16 characters or more than 100 UTF-16 characters.
    ///
    /// Returns a [`CreateGuildChannelErrorType::RateLimitPerUserInvalid`] error
    /// type when the seconds of the rate limit per user is more than 21600.
    ///
    /// Returns a [`CreateGuildChannelErrorType::TopicInvalid`] error type when
    /// the length of the topic is more than 1024 UTF-16 characters.
    ///
    /// [`CreateGuildChannelErrorType::NameInvalid`]: crate::request::guild::create_guild_channel::CreateGuildChannelErrorType::NameInvalid
    /// [`CreateGuildChannelErrorType::RateLimitPerUserInvalid`]: crate::request::guild::create_guild_channel::CreateGuildChannelErrorType::RateLimitPerUserInvalid
    /// [`CreateGuildChannelErrorType::TopicInvalid`]: crate::request::guild::create_guild_channel::CreateGuildChannelErrorType::TopicInvalid
    pub fn create_guild_channel<'a>(
        &'a self,
        guild_id: GuildId,
        name: &'a str,
    ) -> Result<CreateGuildChannel<'a>, CreateGuildChannelError> {
        CreateGuildChannel::new(self, guild_id, name)
    }

    /// Modify the positions of the channels.
    ///
    /// The minimum amount of channels to modify, is a swap between two channels.
    ///
    /// This function accepts an `Iterator` of `(ChannelId, u64)`. It also
    /// accepts an `Iterator` of `Position`, which has extra fields.
    pub const fn update_guild_channel_positions<'a>(
        &'a self,
        guild_id: GuildId,
        channel_positions: &'a [Position],
    ) -> UpdateGuildChannelPositions<'a> {
        UpdateGuildChannelPositions::new(self, guild_id, channel_positions)
    }

    /// Get the guild widget.
    ///
    /// Refer to [the discord docs] for more information.
    ///
    /// [the discord docs]: https://discord.com/developers/docs/resources/guild#get-guild-widget
    pub const fn guild_widget(&self, guild_id: GuildId) -> GetGuildWidget<'_> {
        GetGuildWidget::new(self, guild_id)
    }

    /// Modify the guild widget.
    pub const fn update_guild_widget(&self, guild_id: GuildId) -> UpdateGuildWidget<'_> {
        UpdateGuildWidget::new(self, guild_id)
    }

    /// Get the guild's integrations.
    pub const fn guild_integrations(&self, guild_id: GuildId) -> GetGuildIntegrations<'_> {
        GetGuildIntegrations::new(self, guild_id)
    }

    /// Delete an integration for a guild, by the integration's id.
    pub const fn delete_guild_integration(
        &self,
        guild_id: GuildId,
        integration_id: IntegrationId,
    ) -> DeleteGuildIntegration<'_> {
        DeleteGuildIntegration::new(self, guild_id, integration_id)
    }

    /// Get information about the invites of a guild.
    ///
    /// Requires the [`MANAGE_GUILD`] permission.
    ///
    /// [`MANAGE_GUILD`]: twilight_model::guild::Permissions::MANAGE_GUILD
    pub const fn guild_invites(&self, guild_id: GuildId) -> GetGuildInvites<'_> {
        GetGuildInvites::new(self, guild_id)
    }

    /// Get the members of a guild, by id.
    ///
    /// The upper limit to this request is 1000. If more than 1000 members are needed, the requests
    /// must be chained. Discord defaults the limit to 1.
    ///
    /// # Examples
    ///
    /// Get the first 500 members of guild `100` after user ID `3000`:
    ///
    /// ```no_run
    /// # use twilight_http::Client;
    /// use twilight_model::id::{GuildId, UserId};
    /// #
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("my token".to_owned());
    /// #
    /// let guild_id = GuildId::new(100).expect("non zero");
    /// let user_id = UserId::new(3000).expect("non zero");
    /// let members = client.guild_members(guild_id).after(user_id).exec().await?;
    /// # Ok(()) }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns a [`GetGuildMembersErrorType::LimitInvalid`] error type if the
    /// limit is invalid.
    ///
    /// [`GetGuildMembersErrorType::LimitInvalid`]: crate::request::guild::member::get_guild_members::GetGuildMembersErrorType::LimitInvalid
    pub const fn guild_members(&self, guild_id: GuildId) -> GetGuildMembers<'_> {
        GetGuildMembers::new(self, guild_id)
    }

    /// Search the members of a specific guild by a query.
    ///
    /// The upper limit to this request is 1000. Discord defaults the limit to 1.
    ///
    /// # Examples
    ///
    /// Get the first 10 members of guild `100` matching `Wumpus`:
    ///
    /// ```no_run
    /// use twilight_http::Client;
    /// use twilight_model::id::GuildId;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("my token".to_owned());
    ///
    /// let guild_id = GuildId::new(100).expect("non zero");
    /// let members = client.search_guild_members(guild_id, "Wumpus")
    ///     .limit(10)?
    ///     .exec()
    ///     .await?;
    /// # Ok(()) }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns a [`SearchGuildMembersErrorType::LimitInvalid`] error type if
    /// the limit is invalid.
    ///
    /// [`GUILD_MEMBERS`]: twilight_model::gateway::Intents::GUILD_MEMBERS
    /// [`SearchGuildMembersErrorType::LimitInvalid`]: crate::request::guild::member::search_guild_members::SearchGuildMembersErrorType::LimitInvalid
    pub const fn search_guild_members<'a>(
        &'a self,
        guild_id: GuildId,
        query: &'a str,
    ) -> SearchGuildMembers<'a> {
        SearchGuildMembers::new(self, guild_id, query)
    }

    /// Get a member of a guild, by their id.
    pub const fn guild_member(&self, guild_id: GuildId, user_id: UserId) -> GetMember<'_> {
        GetMember::new(self, guild_id, user_id)
    }

    /// Add a user to a guild.
    ///
    /// An access token for the user with `guilds.join` scope is required. All
    /// other fields are optional. Refer to [the discord docs] for more
    /// information.
    ///
    /// # Errors
    ///
    /// Returns [`AddGuildMemberErrorType::NicknameInvalid`] if the nickname is
    /// too short or too long.
    ///
    /// [`AddGuildMemberErrorType::NickNameInvalid`]: crate::request::guild::member::add_guild_member::AddGuildMemberErrorType::NicknameInvalid
    ///
    /// [the discord docs]: https://discord.com/developers/docs/resources/guild#add-guild-member
    pub const fn add_guild_member<'a>(
        &'a self,
        guild_id: GuildId,
        user_id: UserId,
        access_token: &'a str,
    ) -> AddGuildMember<'a> {
        AddGuildMember::new(self, guild_id, user_id, access_token)
    }

    /// Kick a member from a guild.
    pub const fn remove_guild_member(
        &self,
        guild_id: GuildId,
        user_id: UserId,
    ) -> RemoveMember<'_> {
        RemoveMember::new(self, guild_id, user_id)
    }

    /// Update a guild member.
    ///
    /// All fields are optional. Refer to [the discord docs] for more information.
    ///
    /// # Examples
    ///
    /// Update a member's nickname to "pinky pie" and server mute them:
    ///
    /// ```no_run
    /// use std::env;
    /// use twilight_http::Client;
    /// use twilight_model::id::{GuildId, UserId};
    ///
    /// # #[tokio::main] async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new(env::var("DISCORD_TOKEN")?);
    /// let member = client.update_guild_member(GuildId::new(1).expect("non zero"), UserId::new(2).expect("non zero"))
    ///     .mute(true)
    ///     .nick(Some("pinkie pie"))?
    ///     .exec()
    ///     .await?
    ///     .model()
    ///     .await?;
    ///
    /// println!("user {} now has the nickname '{:?}'", member.user.id, member.nick);
    /// # Ok(()) }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`UpdateGuildMemberErrorType::NicknameInvalid`] if the nickname length is too short or too
    /// long.
    ///
    /// [`UpdateGuildMemberErrorType::NicknameInvalid`]: crate::request::guild::member::update_guild_member::UpdateGuildMemberErrorType::NicknameInvalid
    ///
    /// [the discord docs]: https://discord.com/developers/docs/resources/guild#modify-guild-member
    pub const fn update_guild_member(
        &self,
        guild_id: GuildId,
        user_id: UserId,
    ) -> UpdateGuildMember<'_> {
        UpdateGuildMember::new(self, guild_id, user_id)
    }

    /// Add a role to a member in a guild.
    ///
    /// # Examples
    ///
    /// In guild `1`, add role `2` to user `3`, for the reason `"test"`:
    ///
    /// ```no_run
    /// # use twilight_http::{request::AuditLogReason, Client};
    /// use twilight_model::id::{GuildId, RoleId, UserId};
    /// #
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("my token".to_owned());
    /// #
    /// let guild_id = GuildId::new(1).expect("non zero");
    /// let role_id = RoleId::new(2).expect("non zero");
    /// let user_id = UserId::new(3).expect("non zero");
    ///
    /// client.add_guild_member_role(guild_id, user_id, role_id)
    ///     .reason("test")?
    ///     .exec()
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub const fn add_guild_member_role(
        &self,
        guild_id: GuildId,
        user_id: UserId,
        role_id: RoleId,
    ) -> AddRoleToMember<'_> {
        AddRoleToMember::new(self, guild_id, user_id, role_id)
    }

    /// Remove a role from a member in a guild, by id.
    pub const fn remove_guild_member_role(
        &self,
        guild_id: GuildId,
        user_id: UserId,
        role_id: RoleId,
    ) -> RemoveRoleFromMember<'_> {
        RemoveRoleFromMember::new(self, guild_id, user_id, role_id)
    }

    /// For public guilds, get the guild preview.
    ///
    /// This works even if the user is not in the guild.
    pub const fn guild_preview(&self, guild_id: GuildId) -> GetGuildPreview<'_> {
        GetGuildPreview::new(self, guild_id)
    }

    /// Get the counts of guild members to be pruned.
    pub const fn guild_prune_count(&self, guild_id: GuildId) -> GetGuildPruneCount<'_> {
        GetGuildPruneCount::new(self, guild_id)
    }

    /// Begin a guild prune.
    ///
    /// Refer to [the discord docs] for more information.
    ///
    /// [the discord docs]: https://discord.com/developers/docs/resources/guild#begin-guild-prune
    pub const fn create_guild_prune(&self, guild_id: GuildId) -> CreateGuildPrune<'_> {
        CreateGuildPrune::new(self, guild_id)
    }

    /// Get a guild's vanity url, if there is one.
    pub const fn guild_vanity_url(&self, guild_id: GuildId) -> GetGuildVanityUrl<'_> {
        GetGuildVanityUrl::new(self, guild_id)
    }

    /// Get voice region data for the guild.
    ///
    /// Can return VIP servers if the guild is VIP-enabled.
    pub const fn guild_voice_regions(&self, guild_id: GuildId) -> GetGuildVoiceRegions<'_> {
        GetGuildVoiceRegions::new(self, guild_id)
    }

    /// Get the webhooks of a guild.
    pub const fn guild_webhooks(&self, guild_id: GuildId) -> GetGuildWebhooks<'_> {
        GetGuildWebhooks::new(self, guild_id)
    }

    /// Get the guild's welcome screen.
    pub const fn guild_welcome_screen(&self, guild_id: GuildId) -> GetGuildWelcomeScreen<'_> {
        GetGuildWelcomeScreen::new(self, guild_id)
    }

    /// Update the guild's welcome screen.
    ///
    /// Requires the [`MANAGE_GUILD`] permission.
    ///
    /// [`MANAGE_GUILD`]: twilight_model::guild::Permissions::MANAGE_GUILD
    pub const fn update_guild_welcome_screen(
        &self,
        guild_id: GuildId,
    ) -> UpdateGuildWelcomeScreen<'_> {
        UpdateGuildWelcomeScreen::new(self, guild_id)
    }

    /// Get information about an invite by its code.
    ///
    /// If [`with_counts`] is called, the returned invite will contain
    /// approximate member counts.  If [`with_expiration`] is called, it will
    /// contain the expiration date.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use twilight_http::Client;
    /// #
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("my token".to_owned());
    /// #
    /// let invite = client
    ///     .invite("code")
    ///     .with_counts()
    ///     .exec()
    ///     .await?;
    /// # Ok(()) }
    /// ```
    ///
    /// [`with_counts`]: crate::request::channel::invite::GetInvite::with_counts
    /// [`with_expiration`]: crate::request::channel::invite::GetInvite::with_expiration
    pub const fn invite<'a>(&'a self, code: &'a str) -> GetInvite<'a> {
        GetInvite::new(self, code)
    }

    /// Create an invite, with options.
    ///
    /// Requires the [`CREATE_INVITE`] permission.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use twilight_http::Client;
    /// # use twilight_model::id::ChannelId;
    /// #
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("my token".to_owned());
    /// #
    /// let channel_id = ChannelId::new(123).expect("non zero");
    /// let invite = client
    ///     .create_invite(channel_id)
    ///     .max_uses(3)?
    ///     .exec()
    ///     .await?;
    /// # Ok(()) }
    /// ```
    ///
    /// [`CREATE_INVITE`]: twilight_model::guild::Permissions::CREATE_INVITE
    pub const fn create_invite(&self, channel_id: ChannelId) -> CreateInvite<'_> {
        CreateInvite::new(self, channel_id)
    }

    /// Delete an invite by its code.
    ///
    /// Requires the [`MANAGE_CHANNELS`] permission on the channel this invite
    /// belongs to, or [`MANAGE_GUILD`] to remove any invite across the guild.
    ///
    /// [`MANAGE_CHANNELS`]: twilight_model::guild::Permissions::MANAGE_CHANNELS
    /// [`MANAGE_GUILD`]: twilight_model::guild::Permissions::MANAGE_GUILD
    pub const fn delete_invite<'a>(&'a self, code: &'a str) -> DeleteInvite<'a> {
        DeleteInvite::new(self, code)
    }

    /// Get a message by [`ChannelId`] and [`MessageId`].
    pub const fn message(&self, channel_id: ChannelId, message_id: MessageId) -> GetMessage<'_> {
        GetMessage::new(self, channel_id, message_id)
    }

    /// Send a message to a channel.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use twilight_http::Client;
    /// # use twilight_model::id::ChannelId;
    /// #
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("my token".to_owned());
    /// #
    /// let channel_id = ChannelId::new(123).expect("non zero");
    /// let message = client
    ///     .create_message(channel_id)
    ///     .content("Twilight is best pony")?
    ///     .tts(true)
    ///     .exec()
    ///     .await?;
    /// # Ok(()) }
    /// ```
    ///
    /// # Errors
    ///
    /// The method [`content`] returns
    /// [`CreateMessageErrorType::ContentInvalid`] if the content is over 2000
    /// UTF-16 characters.
    ///
    /// The method [`embeds`] returns
    /// [`CreateMessageErrorType::EmbedTooLarge`] if the length of the embed
    /// is over 6000 characters.
    ///
    /// [`content`]: crate::request::channel::message::create_message::CreateMessage::content
    /// [`embeds`]: crate::request::channel::message::create_message::CreateMessage::embeds
    /// [`CreateMessageErrorType::ContentInvalid`]:
    /// crate::request::channel::message::create_message::CreateMessageErrorType::ContentInvalid
    /// [`CreateMessageErrorType::EmbedTooLarge`]:
    /// crate::request::channel::message::create_message::CreateMessageErrorType::EmbedTooLarge
    pub const fn create_message(&self, channel_id: ChannelId) -> CreateMessage<'_> {
        CreateMessage::new(self, channel_id)
    }

    /// Delete a message by [`ChannelId`] and [`MessageId`].
    pub const fn delete_message(
        &self,
        channel_id: ChannelId,
        message_id: MessageId,
    ) -> DeleteMessage<'_> {
        DeleteMessage::new(self, channel_id, message_id)
    }

    /// Delete messages by [`ChannelId`] and Vec<[`MessageId`]>.
    ///
    /// The vec count can be between 2 and 100. If the supplied [`MessageId`]s are invalid, they
    /// still count towards the lower and upper limits. This method will not delete messages older
    /// than two weeks. Refer to [the discord docs] for more information.
    ///
    /// [the discord docs]: https://discord.com/developers/docs/resources/channel#bulk-delete-messages
    pub const fn delete_messages<'a>(
        &'a self,
        channel_id: ChannelId,
        message_ids: &'a [MessageId],
    ) -> DeleteMessages<'a> {
        DeleteMessages::new(self, channel_id, message_ids)
    }

    /// Update a message by [`ChannelId`] and [`MessageId`].
    ///
    /// You can pass `None` to any of the methods to remove the associated field.
    /// For example, if you have a message with an embed you want to remove, you can
    /// use `.[embed](None)` to remove the embed.
    ///
    /// # Examples
    ///
    /// Replace the content with `"test update"`:
    ///
    /// ```no_run
    /// use twilight_http::Client;
    /// use twilight_model::id::{ChannelId, MessageId};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("my token".to_owned());
    /// client.update_message(ChannelId::new(1).expect("non zero"), MessageId::new(2).expect("non zero"))
    ///     .content(Some("test update"))?
    ///     .exec()
    ///     .await?;
    /// # Ok(()) }
    /// ```
    ///
    /// Remove the message's content:
    ///
    /// ```no_run
    /// # use twilight_http::Client;
    /// # use twilight_model::id::{ChannelId, MessageId};
    /// #
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("my token".to_owned());
    /// client.update_message(ChannelId::new(1).expect("non zero"), MessageId::new(2).expect("non zero"))
    ///     .content(None)?
    ///     .exec()
    ///     .await?;
    /// # Ok(()) }
    /// ```
    ///
    /// [embed]: Self::embed
    pub const fn update_message(
        &self,
        channel_id: ChannelId,
        message_id: MessageId,
    ) -> UpdateMessage<'_> {
        UpdateMessage::new(self, channel_id, message_id)
    }

    /// Crosspost a message by [`ChannelId`] and [`MessageId`].
    pub const fn crosspost_message(
        &self,
        channel_id: ChannelId,
        message_id: MessageId,
    ) -> CrosspostMessage<'_> {
        CrosspostMessage::new(self, channel_id, message_id)
    }

    /// Get the pins of a channel.
    pub const fn pins(&self, channel_id: ChannelId) -> GetPins<'_> {
        GetPins::new(self, channel_id)
    }

    /// Create a new pin in a channel, by ID.
    pub const fn create_pin(&self, channel_id: ChannelId, message_id: MessageId) -> CreatePin<'_> {
        CreatePin::new(self, channel_id, message_id)
    }

    /// Delete a pin in a channel, by ID.
    pub const fn delete_pin(&self, channel_id: ChannelId, message_id: MessageId) -> DeletePin<'_> {
        DeletePin::new(self, channel_id, message_id)
    }

    /// Get a list of users that reacted to a message with an `emoji`.
    ///
    /// This endpoint is limited to 100 users maximum, so if a message has more than 100 reactions,
    /// requests must be chained until all reactions are retireved.
    pub const fn reactions<'a>(
        &'a self,
        channel_id: ChannelId,
        message_id: MessageId,
        emoji: &'a RequestReactionType<'a>,
    ) -> GetReactions<'a> {
        GetReactions::new(self, channel_id, message_id, emoji)
    }

    /// Create a reaction in a [`ChannelId`] on a [`MessageId`].
    ///
    /// The reaction must be a variant of [`RequestReactionType`].
    ///
    /// # Examples
    /// ```no_run
    /// # use twilight_http::{Client, request::channel::reaction::RequestReactionType};
    /// # use twilight_model::{
    /// #     id::{ChannelId, MessageId},
    /// # };
    /// #
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("my token".to_owned());
    /// #
    /// let channel_id = ChannelId::new(123).expect("non zero");
    /// let message_id = MessageId::new(456).expect("non zero");
    /// let emoji = RequestReactionType::Unicode { name: "🌃" };
    ///
    /// let reaction = client
    ///     .create_reaction(channel_id, message_id, &emoji)
    ///     .exec()
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub const fn create_reaction<'a>(
        &'a self,
        channel_id: ChannelId,
        message_id: MessageId,
        emoji: &'a RequestReactionType<'a>,
    ) -> CreateReaction<'a> {
        CreateReaction::new(self, channel_id, message_id, emoji)
    }

    /// Delete the current user's (`@me`) reaction on a message.
    pub const fn delete_current_user_reaction<'a>(
        &'a self,
        channel_id: ChannelId,
        message_id: MessageId,
        emoji: &'a RequestReactionType<'a>,
    ) -> DeleteReaction<'a> {
        DeleteReaction::new(self, channel_id, message_id, emoji, TargetUser::Current)
    }

    /// Delete a reaction by a user on a message.
    pub const fn delete_reaction<'a>(
        &'a self,
        channel_id: ChannelId,
        message_id: MessageId,
        emoji: &'a RequestReactionType<'a>,
        user_id: UserId,
    ) -> DeleteReaction<'a> {
        DeleteReaction::new(self, channel_id, message_id, emoji, TargetUser::Id(user_id))
    }

    /// Remove all reactions on a message of an emoji.
    pub const fn delete_all_reaction<'a>(
        &'a self,
        channel_id: ChannelId,
        message_id: MessageId,
        emoji: &'a RequestReactionType<'a>,
    ) -> DeleteAllReaction<'a> {
        DeleteAllReaction::new(self, channel_id, message_id, emoji)
    }

    /// Delete all reactions by all users on a message.
    pub const fn delete_all_reactions(
        &self,
        channel_id: ChannelId,
        message_id: MessageId,
    ) -> DeleteAllReactions<'_> {
        DeleteAllReactions::new(self, channel_id, message_id)
    }

    /// Fire a Typing Start event in the channel.
    pub const fn create_typing_trigger(&self, channel_id: ChannelId) -> CreateTypingTrigger<'_> {
        CreateTypingTrigger::new(self, channel_id)
    }

    /// Create a group DM.
    ///
    /// This endpoint is limited to 10 active group DMs.
    pub const fn create_private_channel(&self, recipient_id: UserId) -> CreatePrivateChannel<'_> {
        CreatePrivateChannel::new(self, recipient_id)
    }

    /// Get the roles of a guild.
    pub const fn roles(&self, guild_id: GuildId) -> GetGuildRoles<'_> {
        GetGuildRoles::new(self, guild_id)
    }

    /// Create a role in a guild.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use twilight_http::Client;
    /// use twilight_model::id::GuildId;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("my token".to_owned());
    /// let guild_id = GuildId::new(234).expect("non zero");
    ///
    /// client.create_role(guild_id)
    ///     .color(0xd90083)
    ///     .name("Bright Pink")
    ///     .exec()
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub const fn create_role(&self, guild_id: GuildId) -> CreateRole<'_> {
        CreateRole::new(self, guild_id)
    }

    /// Delete a role in a guild, by id.
    pub const fn delete_role(&self, guild_id: GuildId, role_id: RoleId) -> DeleteRole<'_> {
        DeleteRole::new(self, guild_id, role_id)
    }

    /// Update a role by guild id and its id.
    pub const fn update_role(&self, guild_id: GuildId, role_id: RoleId) -> UpdateRole<'_> {
        UpdateRole::new(self, guild_id, role_id)
    }

    /// Modify the position of the roles.
    ///
    /// The minimum amount of roles to modify, is a swap between two roles.
    pub const fn update_role_positions<'a>(
        &'a self,
        guild_id: GuildId,
        roles: &'a [(RoleId, u64)],
    ) -> UpdateRolePositions<'a> {
        UpdateRolePositions::new(self, guild_id, roles)
    }

    /// Create a new stage instance associated with a stage channel.
    ///
    /// Requires the user to be a moderator of the stage channel.
    ///
    /// # Errors
    ///
    /// Returns a [`CreateStageInstanceError`] of type [`InvalidTopic`] when the
    /// topic is not between 1 and 120 characters in length.
    ///
    /// [`InvalidTopic`]: crate::request::channel::stage::create_stage_instance::CreateStageInstanceErrorType::InvalidTopic
    pub fn create_stage_instance<'a>(
        &'a self,
        channel_id: ChannelId,
        topic: &'a str,
    ) -> Result<CreateStageInstance<'a>, CreateStageInstanceError> {
        CreateStageInstance::new(self, channel_id, topic)
    }

    /// Gets the stage instance associated with a stage channel, if it exists.
    pub const fn stage_instance(&self, channel_id: ChannelId) -> GetStageInstance<'_> {
        GetStageInstance::new(self, channel_id)
    }

    /// Update fields of an existing stage instance.
    ///
    /// Requires the user to be a moderator of the stage channel.
    pub const fn update_stage_instance(&self, channel_id: ChannelId) -> UpdateStageInstance<'_> {
        UpdateStageInstance::new(self, channel_id)
    }

    /// Delete the stage instance of a stage channel.
    ///
    /// Requires the user to be a moderator of the stage channel.
    pub const fn delete_stage_instance(&self, channel_id: ChannelId) -> DeleteStageInstance<'_> {
        DeleteStageInstance::new(self, channel_id)
    }

    /// Create a new guild based on a template.
    ///
    /// This endpoint can only be used by bots in less than 10 guilds.
    ///
    /// # Errors
    ///
    /// Returns a [`CreateGuildFromTemplateErrorType::NameInvalid`] error type
    /// if the name is invalid.
    ///
    /// [`CreateGuildFromTemplateErrorType::NameInvalid`]: crate::request::template::create_guild_from_template::CreateGuildFromTemplateErrorType::NameInvalid
    pub fn create_guild_from_template<'a>(
        &'a self,
        template_code: &'a str,
        name: &'a str,
    ) -> Result<CreateGuildFromTemplate<'a>, CreateGuildFromTemplateError> {
        CreateGuildFromTemplate::new(self, template_code, name)
    }

    /// Create a template from the current state of the guild.
    ///
    /// Requires the `MANAGE_GUILD` permission. The name must be at least 1 and
    /// at most 100 characters in length.
    ///
    /// # Errors
    ///
    /// Returns a [`CreateTemplateErrorType::NameInvalid`] error type if the
    /// name is invalid.
    ///
    /// [`CreateTemplateErrorType::NameInvalid`]: crate::request::template::create_template::CreateTemplateErrorType::NameInvalid
    pub fn create_template<'a>(
        &'a self,
        guild_id: GuildId,
        name: &'a str,
    ) -> Result<CreateTemplate<'a>, CreateTemplateError> {
        CreateTemplate::new(self, guild_id, name)
    }

    /// Delete a template by ID and code.
    pub const fn delete_template<'a>(
        &'a self,
        guild_id: GuildId,
        template_code: &'a str,
    ) -> DeleteTemplate<'a> {
        DeleteTemplate::new(self, guild_id, template_code)
    }

    /// Get a template by its code.
    pub const fn get_template<'a>(&'a self, template_code: &'a str) -> GetTemplate<'a> {
        GetTemplate::new(self, template_code)
    }

    /// Get a list of templates in a guild, by ID.
    pub const fn get_templates(&self, guild_id: GuildId) -> GetTemplates<'_> {
        GetTemplates::new(self, guild_id)
    }

    /// Sync a template to the current state of the guild, by ID and code.
    pub const fn sync_template<'a>(
        &'a self,
        guild_id: GuildId,
        template_code: &'a str,
    ) -> SyncTemplate<'a> {
        SyncTemplate::new(self, guild_id, template_code)
    }

    /// Update the template's metadata, by ID and code.
    pub const fn update_template<'a>(
        &'a self,
        guild_id: GuildId,
        template_code: &'a str,
    ) -> UpdateTemplate<'a> {
        UpdateTemplate::new(self, guild_id, template_code)
    }

    /// Get a user's information by id.
    pub const fn user(&self, user_id: UserId) -> GetUser<'_> {
        GetUser::new(self, user_id)
    }

    /// Update another user's voice state.
    ///
    /// # Caveats
    ///
    /// - `channel_id` must currently point to a stage channel.
    /// - User must already have joined `channel_id`.
    pub const fn update_user_voice_state(
        &self,
        guild_id: GuildId,
        user_id: UserId,
        channel_id: ChannelId,
    ) -> UpdateUserVoiceState<'_> {
        UpdateUserVoiceState::new(self, guild_id, user_id, channel_id)
    }

    /// Get a list of voice regions that can be used when creating a guild.
    pub const fn voice_regions(&self) -> GetVoiceRegions<'_> {
        GetVoiceRegions::new(self)
    }

    /// Get a webhook by ID.
    pub const fn webhook(&self, id: WebhookId) -> GetWebhook<'_> {
        GetWebhook::new(self, id)
    }

    /// Create a webhook in a channel.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use twilight_http::Client;
    /// # use twilight_model::id::ChannelId;
    /// #
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("my token".to_owned());
    /// let channel_id = ChannelId::new(123).expect("non zero");
    ///
    /// let webhook = client
    ///     .create_webhook(channel_id, "Twily Bot")
    ///     .exec()
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub const fn create_webhook<'a>(
        &'a self,
        channel_id: ChannelId,
        name: &'a str,
    ) -> CreateWebhook<'a> {
        CreateWebhook::new(self, channel_id, name)
    }

    /// Delete a webhook by its ID.
    pub const fn delete_webhook(&self, id: WebhookId) -> DeleteWebhook<'_> {
        DeleteWebhook::new(self, id)
    }

    /// Update a webhook by ID.
    pub const fn update_webhook(&self, webhook_id: WebhookId) -> UpdateWebhook<'_> {
        UpdateWebhook::new(self, webhook_id)
    }

    /// Update a webhook, with a token, by ID.
    pub const fn update_webhook_with_token<'a>(
        &'a self,
        webhook_id: WebhookId,
        token: &'a str,
    ) -> UpdateWebhookWithToken<'a> {
        UpdateWebhookWithToken::new(self, webhook_id, token)
    }

    /// Executes a webhook, sending a message to its channel.
    ///
    /// You can only specify one of [`content`], [`embeds`], or [`files`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use twilight_http::Client;
    /// # use twilight_model::id::WebhookId;
    /// #
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("my token".to_owned());
    /// let id = WebhookId::new(432).expect("non zero");
    /// #
    /// let webhook = client
    ///     .execute_webhook(id, "webhook token")
    ///     .content("Pinkie...")
    ///     .exec()
    ///     .await?;
    /// # Ok(()) }
    /// ```
    ///
    /// [`content`]: crate::request::channel::webhook::ExecuteWebhook::content
    /// [`embeds`]: crate::request::channel::webhook::ExecuteWebhook::embeds
    /// [`files`]: crate::request::channel::webhook::ExecuteWebhook::files
    pub const fn execute_webhook<'a>(
        &'a self,
        webhook_id: WebhookId,
        token: &'a str,
    ) -> ExecuteWebhook<'a> {
        ExecuteWebhook::new(self, webhook_id, token)
    }

    /// Get a webhook message by [`WebhookId`], token, and [`MessageId`].
    ///
    /// [`WebhookId`]: twilight_model::id::WebhookId
    /// [`MessageId`]: twilight_model::id::MessageId
    pub const fn webhook_message<'a>(
        &'a self,
        webhook_id: WebhookId,
        token: &'a str,
        message_id: MessageId,
    ) -> GetWebhookMessage<'a> {
        GetWebhookMessage::new(self, webhook_id, token, message_id)
    }

    /// Update a message executed by a webhook.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use twilight_http::Client;
    /// use twilight_model::id::{MessageId, WebhookId};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("token".to_owned());
    /// client.update_webhook_message(WebhookId::new(1).expect("non zero"), "token here", MessageId::new(2).expect("non zero"))
    ///     .content(Some("new message content"))?
    ///     .exec()
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub const fn update_webhook_message<'a>(
        &'a self,
        webhook_id: WebhookId,
        token: &'a str,
        message_id: MessageId,
    ) -> UpdateWebhookMessage<'a> {
        UpdateWebhookMessage::new(self, webhook_id, token, message_id)
    }

    /// Delete a message executed by a webhook.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use twilight_http::Client;
    /// use twilight_model::id::{MessageId, WebhookId};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("token".to_owned());
    /// client
    ///     .delete_webhook_message(WebhookId::new(1).expect("non zero"), "token here", MessageId::new(2).expect("non zero"))
    ///     .exec()
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub const fn delete_webhook_message<'a>(
        &'a self,
        webhook_id: WebhookId,
        token: &'a str,
        message_id: MessageId,
    ) -> DeleteWebhookMessage<'a> {
        DeleteWebhookMessage::new(self, webhook_id, token, message_id)
    }

    /// Respond to an interaction, by ID and token.
    pub const fn interaction_callback<'a>(
        &'a self,
        interaction_id: InteractionId,
        interaction_token: &'a str,
        response: &'a InteractionResponse,
    ) -> InteractionCallback<'a> {
        InteractionCallback::new(self, interaction_id, interaction_token, response)
    }

    /// Edit the original message, by its token.
    ///
    /// # Errors
    ///
    /// Returns an [`InteractionErrorType::ApplicationIdNotPresent`]
    /// error type if an application ID has not been configured via
    /// [`Client::set_application_id`].
    pub fn update_interaction_original<'a>(
        &'a self,
        interaction_token: &'a str,
    ) -> Result<UpdateOriginalResponse<'a>, InteractionError> {
        let application_id = self.application_id().ok_or(InteractionError {
            kind: InteractionErrorType::ApplicationIdNotPresent,
        })?;

        Ok(UpdateOriginalResponse::new(
            self,
            application_id,
            interaction_token,
        ))
    }

    /// Delete the original message, by its token.
    ///
    /// # Errors
    ///
    /// Returns an [`InteractionErrorType::ApplicationIdNotPresent`]
    /// error type if an application ID has not been configured via
    /// [`Client::set_application_id`].
    pub fn delete_interaction_original<'a>(
        &'a self,
        interaction_token: &'a str,
    ) -> Result<DeleteOriginalResponse<'a>, InteractionError> {
        let application_id = self.application_id().ok_or(InteractionError {
            kind: InteractionErrorType::ApplicationIdNotPresent,
        })?;

        Ok(DeleteOriginalResponse::new(
            self,
            application_id,
            interaction_token,
        ))
    }

    /// Create a followup message, by an interaction token.
    ///
    /// # Errors
    ///
    /// Returns an [`InteractionErrorType::ApplicationIdNotPresent`]
    /// error type if an application ID has not been configured via
    /// [`Client::set_application_id`].
    pub fn create_followup_message<'a>(
        &'a self,
        interaction_token: &'a str,
    ) -> Result<CreateFollowupMessage<'a>, InteractionError> {
        let application_id = self.application_id().ok_or(InteractionError {
            kind: InteractionErrorType::ApplicationIdNotPresent,
        })?;

        Ok(CreateFollowupMessage::new(
            self,
            application_id,
            interaction_token,
        ))
    }

    /// Edit a followup message, by an interaction token.
    ///
    /// # Errors
    ///
    /// Returns an [`InteractionErrorType::ApplicationIdNotPresent`]
    /// error type if an application ID has not been configured via
    /// [`Client::set_application_id`].
    pub fn update_followup_message<'a>(
        &'a self,
        interaction_token: &'a str,
        message_id: MessageId,
    ) -> Result<UpdateFollowupMessage<'a>, InteractionError> {
        let application_id = self.application_id().ok_or(InteractionError {
            kind: InteractionErrorType::ApplicationIdNotPresent,
        })?;

        Ok(UpdateFollowupMessage::new(
            self,
            application_id,
            interaction_token,
            message_id,
        ))
    }

    /// Delete a followup message by interaction token and the message's ID.
    ///
    /// # Errors
    ///
    /// Returns an [`InteractionErrorType::ApplicationIdNotPresent`]
    /// error type if an application ID has not been configured via
    /// [`Client::set_application_id`].
    pub fn delete_followup_message<'a>(
        &'a self,
        interaction_token: &'a str,
        message_id: MessageId,
    ) -> Result<DeleteFollowupMessage<'a>, InteractionError> {
        let application_id = self.application_id().ok_or(InteractionError {
            kind: InteractionErrorType::ApplicationIdNotPresent,
        })?;

        Ok(DeleteFollowupMessage::new(
            self,
            application_id,
            interaction_token,
            message_id,
        ))
    }

    /// Create a new command in a guild.
    ///
    /// The name must be between 3 and 32 characters in length, and the
    /// description must be between 1 and 100 characters in length. Creating a
    /// guild command with the same name as an already-existing guild command in
    /// the same guild will overwrite the old command. See [the discord docs]
    /// for more information.
    ///
    /// # Errors
    ///
    /// Returns an [`InteractionErrorType::ApplicationIdNotPresent`]
    /// error type if an application ID has not been configured via
    /// [`Client::set_application_id`].
    ///
    /// Returns an [`InteractionErrorType::CommandNameValidationFailed`]
    /// error type if the command name is not between 3 and 32 characters.
    ///
    /// Returns an [`InteractionErrorType::CommandDescriptionValidationFailed`]
    /// error type if the command description is not between 1 and
    /// 100 characters.
    ///
    /// [the discord docs]: https://discord.com/developers/docs/interactions/slash-commands#create-guild-application-command
    pub fn create_guild_command<'a>(
        &'a self,
        guild_id: GuildId,
        name: &'a str,
        description: &'a str,
    ) -> Result<CreateGuildCommand<'a>, InteractionError> {
        let application_id = self.application_id().ok_or(InteractionError {
            kind: InteractionErrorType::ApplicationIdNotPresent,
        })?;

        CreateGuildCommand::new(&self, application_id, guild_id, name, description)
    }

    /// Fetch all commands for a guild, by ID.
    ///
    /// # Errors
    ///
    /// Returns an [`InteractionErrorType::ApplicationIdNotPresent`]
    /// error type if an application ID has not been configured via
    /// [`Client::set_application_id`].
    pub fn get_guild_commands(
        &self,
        guild_id: GuildId,
    ) -> Result<GetGuildCommands<'_>, InteractionError> {
        let application_id = self.application_id().ok_or(InteractionError {
            kind: InteractionErrorType::ApplicationIdNotPresent,
        })?;

        Ok(GetGuildCommands::new(self, application_id, guild_id))
    }

    /// Edit a command in a guild, by ID.
    ///
    /// You must specify a name and description. See [the discord docs] for more
    /// information.
    ///
    /// # Errors
    ///
    /// Returns an [`InteractionErrorType::ApplicationIdNotPresent`]
    /// error type if an application ID has not been configured via
    /// [`Client::set_application_id`].
    ///
    /// [the discord docs]: https://discord.com/developers/docs/interactions/slash-commands#edit-guild-application-command
    pub fn update_guild_command(
        &self,
        guild_id: GuildId,
        command_id: CommandId,
    ) -> Result<UpdateGuildCommand<'_>, InteractionError> {
        let application_id = self.application_id().ok_or(InteractionError {
            kind: InteractionErrorType::ApplicationIdNotPresent,
        })?;

        Ok(UpdateGuildCommand::new(
            self,
            application_id,
            guild_id,
            command_id,
        ))
    }

    /// Delete a command in a guild, by ID.
    ///
    /// # Errors
    ///
    /// Returns an [`InteractionErrorType::ApplicationIdNotPresent`]
    /// error type if an application ID has not been configured via
    /// [`Client::set_application_id`].
    pub fn delete_guild_command(
        &self,
        guild_id: GuildId,
        command_id: CommandId,
    ) -> Result<DeleteGuildCommand<'_>, InteractionError> {
        let application_id = self.application_id().ok_or(InteractionError {
            kind: InteractionErrorType::ApplicationIdNotPresent,
        })?;

        Ok(DeleteGuildCommand::new(
            self,
            application_id,
            guild_id,
            command_id,
        ))
    }

    /// Set a guild's commands.
    ///
    /// This method is idempotent: it can be used on every start, without being
    /// ratelimited if there aren't changes to the commands.
    ///
    /// # Errors
    ///
    /// Returns an [`InteractionErrorType::ApplicationIdNotPresent`]
    /// error type if an application ID has not been configured via
    /// [`Client::set_application_id`].
    pub fn set_guild_commands<'a>(
        &'a self,
        guild_id: GuildId,
        commands: &'a [Command],
    ) -> Result<SetGuildCommands<'a>, InteractionError> {
        let application_id = self.application_id().ok_or(InteractionError {
            kind: InteractionErrorType::ApplicationIdNotPresent,
        })?;

        Ok(SetGuildCommands::new(
            self,
            application_id,
            guild_id,
            commands,
        ))
    }

    /// Create a new global command.
    ///
    /// The name must be between 3 and 32 characters in length, and the
    /// description must be between 1 and 100 characters in length. Creating a
    /// command with the same name as an already-existing global command will
    /// overwrite the old command. See [the discord docs] for more information.
    ///
    /// # Errors
    ///
    /// Returns an [`InteractionErrorType::ApplicationIdNotPresent`]
    /// error type if an application ID has not been configured via
    /// [`Client::set_application_id`].
    ///
    /// Returns an [`InteractionErrorType::CommandNameValidationFailed`]
    /// error type if the command name is not between 3 and 32 characters.
    ///
    /// Returns an [`InteractionErrorType::CommandDescriptionValidationFailed`]
    /// error type if the command description is not between 1 and
    /// 100 characters.
    ///
    /// [the discord docs]: https://discord.com/developers/docs/interactions/slash-commands#create-global-application-command
    pub fn create_global_command<'a>(
        &'a self,
        name: &'a str,
        description: &'a str,
    ) -> Result<CreateGlobalCommand<'a>, InteractionError> {
        let application_id = self.application_id().ok_or(InteractionError {
            kind: InteractionErrorType::ApplicationIdNotPresent,
        })?;

        CreateGlobalCommand::new(self, application_id, name, description)
    }

    /// Fetch all global commands for your application.
    ///
    /// # Errors
    ///
    /// Returns an [`InteractionErrorType::ApplicationIdNotPresent`]
    /// error type if an application ID has not been configured via
    /// [`Client::set_application_id`].
    pub fn get_global_commands(&self) -> Result<GetGlobalCommands<'_>, InteractionError> {
        let application_id = self.application_id().ok_or(InteractionError {
            kind: InteractionErrorType::ApplicationIdNotPresent,
        })?;

        Ok(GetGlobalCommands::new(self, application_id))
    }

    /// Edit a global command, by ID.
    ///
    /// You must specify a name and description. See [the discord docs] for more
    /// information.
    ///
    /// # Errors
    ///
    /// Returns an [`InteractionErrorType::ApplicationIdNotPresent`]
    /// error type if an application ID has not been configured via
    /// [`Client::set_application_id`].
    ///
    /// [the discord docs]: https://discord.com/developers/docs/interactions/slash-commands#edit-global-application-command
    pub fn update_global_command(
        &self,
        command_id: CommandId,
    ) -> Result<UpdateGlobalCommand<'_>, InteractionError> {
        let application_id = self.application_id().ok_or(InteractionError {
            kind: InteractionErrorType::ApplicationIdNotPresent,
        })?;

        Ok(UpdateGlobalCommand::new(self, application_id, command_id))
    }

    /// Delete a global command, by ID.
    ///
    /// # Errors
    ///
    /// Returns an [`InteractionErrorType::ApplicationIdNotPresent`]
    /// error type if an application ID has not been configured via
    /// [`Client::set_application_id`].
    pub fn delete_global_command(
        &self,
        command_id: CommandId,
    ) -> Result<DeleteGlobalCommand<'_>, InteractionError> {
        let application_id = self.application_id().ok_or(InteractionError {
            kind: InteractionErrorType::ApplicationIdNotPresent,
        })?;

        Ok(DeleteGlobalCommand::new(self, application_id, command_id))
    }

    /// Set global commands.
    ///
    /// This method is idempotent: it can be used on every start, without being
    /// ratelimited if there aren't changes to the commands.
    ///
    /// # Errors
    ///
    /// Returns an [`InteractionErrorType::ApplicationIdNotPresent`]
    /// error type if an application ID has not been configured via
    /// [`Client::set_application_id`].
    pub fn set_global_commands<'a>(
        &'a self,
        commands: &'a [Command],
    ) -> Result<SetGlobalCommands<'a>, InteractionError> {
        let application_id = self.application_id().ok_or(InteractionError {
            kind: InteractionErrorType::ApplicationIdNotPresent,
        })?;

        Ok(SetGlobalCommands::new(self, application_id, commands))
    }

    /// Fetch command permissions for a command from the current application
    /// in a guild.
    ///
    /// # Errors
    ///
    /// Returns an [`InteractionErrorType::ApplicationIdNotPresent`]
    /// error type if an application ID has not been configured via
    /// [`Client::set_application_id`].
    pub fn get_command_permissions(
        &self,
        guild_id: GuildId,
        command_id: CommandId,
    ) -> Result<GetCommandPermissions<'_>, InteractionError> {
        let application_id = self.application_id().ok_or(InteractionError {
            kind: InteractionErrorType::ApplicationIdNotPresent,
        })?;

        Ok(GetCommandPermissions::new(
            &self,
            application_id,
            guild_id,
            command_id,
        ))
    }

    /// Fetch command permissions for all commands from the current
    /// application in a guild.
    ///
    /// # Errors
    ///
    /// Returns an [`InteractionErrorType::ApplicationIdNotPresent`]
    /// error type if an application ID has not been configured via
    /// [`Client::set_application_id`].
    pub fn get_guild_command_permissions(
        &self,
        guild_id: GuildId,
    ) -> Result<GetGuildCommandPermissions<'_>, InteractionError> {
        let application_id = self.application_id().ok_or(InteractionError {
            kind: InteractionErrorType::ApplicationIdNotPresent,
        })?;

        Ok(GetGuildCommandPermissions::new(
            self,
            application_id,
            guild_id,
        ))
    }

    /// Update command permissions for a single command in a guild.
    ///
    /// This overwrites the command permissions so the full set of permissions
    /// have to be sent every time.
    ///
    /// # Errors
    ///
    /// Returns an [`InteractionErrorType::ApplicationIdNotPresent`]
    /// error type if an application ID has not been configured via
    /// [`Client::set_application_id`].
    pub fn update_command_permissions<'a>(
        &'a self,
        guild_id: GuildId,
        command_id: CommandId,
        permissions: &'a [CommandPermissions],
    ) -> Result<UpdateCommandPermissions<'a>, InteractionError> {
        let application_id = self.application_id().ok_or(InteractionError {
            kind: InteractionErrorType::ApplicationIdNotPresent,
        })?;

        UpdateCommandPermissions::new(self, application_id, guild_id, command_id, permissions)
    }

    /// Update command permissions for all commands in a guild.
    ///
    /// This overwrites the command permissions so the full set of permissions
    /// have to be sent every time.
    ///
    /// # Errors
    ///
    /// Returns an [`InteractionErrorType::ApplicationIdNotPresent`]
    /// error type if an application ID has not been configured via
    /// [`Client::set_application_id`].
    ///
    /// Returns an [`InteractionErrorType::TooManyCommands`] error type if too
    /// many commands have been provided. The maximum amount is defined by
    /// [`InteractionError::GUILD_COMMAND_LIMIT`].
    pub fn set_command_permissions<'a>(
        &'a self,
        guild_id: GuildId,
        permissions: &'a [(CommandId, CommandPermissions)],
    ) -> Result<SetCommandPermissions<'a>, InteractionError> {
        let application_id = self.application_id().ok_or(InteractionError {
            kind: InteractionErrorType::ApplicationIdNotPresent,
        })?;

        SetCommandPermissions::new(self, application_id, guild_id, permissions)
    }

    /// Execute a request, returning a future resolving to a [`Response`].
    ///
    /// # Errors
    ///
    /// Returns an [`ErrorType::Unauthorized`] error type if the configured
    /// token has become invalid due to expiration, revokation, etc.
    ///
    /// [`Response`]: super::response::Response
    pub fn request<T>(&self, request: Request<'_>) -> ResponseFuture<T> {
        match self.try_request::<T>(request) {
            Ok(future) => future,
            Err(source) => ResponseFuture::error(source),
        }
    }

    #[allow(clippy::too_many_lines)]
    fn try_request<T>(&self, request: Request<'_>) -> Result<ResponseFuture<T>, Error> {
        if self.state.token_invalid.load(Ordering::Relaxed) {
            return Err(Error {
                kind: ErrorType::Unauthorized,
                source: None,
            });
        }

        let Request {
            body,
            form,
            headers: req_headers,
            route,
            use_authorization_token,
        } = request;

        let protocol = if self.state.use_http { "http" } else { "https" };
        let host = self.state.proxy.as_deref().unwrap_or("discord.com");

        let url = format!(
            "{}://{}/api/v{}/{}",
            protocol,
            host,
            API_VERSION,
            route.display()
        );
        #[cfg(feature = "tracing")]
        tracing::debug!("URL: {:?}", url);

        let mut builder = hyper::Request::builder()
            .method(route.method().into_hyper())
            .uri(&url);

        if use_authorization_token {
            if let Some(ref token) = self.state.token {
                let value = HeaderValue::from_str(&token).map_err(|source| {
                    #[allow(clippy::borrow_interior_mutable_const)]
                    let name = AUTHORIZATION.to_string();

                    Error {
                        kind: ErrorType::CreatingHeader { name },
                        source: Some(Box::new(source)),
                    }
                })?;

                if let Some(headers) = builder.headers_mut() {
                    headers.insert(AUTHORIZATION, value);
                }
            }
        }

        let user_agent = HeaderValue::from_static(concat!(
            "DiscordBot (",
            env!("CARGO_PKG_HOMEPAGE"),
            ", ",
            env!("CARGO_PKG_VERSION"),
            ") Twilight-rs",
        ));

        if let Some(headers) = builder.headers_mut() {
            if let Some(form) = &form {
                if let Ok(content_type) = HeaderValue::try_from(form.content_type()) {
                    headers.insert(CONTENT_TYPE, content_type);
                }
            } else if let Some(bytes) = &body {
                let len = bytes.len();
                headers.insert(CONTENT_LENGTH, HeaderValue::from(len));

                let content_type = HeaderValue::from_static("application/json");
                headers.insert(CONTENT_TYPE, content_type);
            }

            headers.insert(USER_AGENT, user_agent);

            if let Some(req_headers) = req_headers {
                for (maybe_name, value) in req_headers {
                    if let Some(name) = maybe_name {
                        headers.insert(name, value);
                    }
                }
            }

            if let Some(default_headers) = &self.state.default_headers {
                for (name, value) in default_headers {
                    headers.insert(name, HeaderValue::from(value));
                }
            }
        }

        let method = route.method();

        let req = if let Some(form) = form {
            let form_bytes = form.build();
            if let Some(headers) = builder.headers_mut() {
                headers.insert(CONTENT_LENGTH, HeaderValue::from(form_bytes.len()));
            };
            builder
                .body(Body::from(form_bytes))
                .map_err(|source| Error {
                    kind: ErrorType::BuildingRequest,
                    source: Some(Box::new(source)),
                })?
        } else if let Some(bytes) = body {
            builder.body(Body::from(bytes)).map_err(|source| Error {
                kind: ErrorType::BuildingRequest,
                source: Some(Box::new(source)),
            })?
        } else if method == Method::Put || method == Method::Post || method == Method::Patch {
            if let Some(headers) = builder.headers_mut() {
                headers.insert(CONTENT_LENGTH, HeaderValue::from(0));
            }

            builder.body(Body::empty()).map_err(|source| Error {
                kind: ErrorType::BuildingRequest,
                source: Some(Box::new(source)),
            })?
        } else {
            builder.body(Body::empty()).map_err(|source| Error {
                kind: ErrorType::BuildingRequest,
                source: Some(Box::new(source)),
            })?
        };

        let inner = self.state.http.request(req);
        let token_invalid = Arc::clone(&self.state.token_invalid);

        // Clippy suggests bad code; an `Option::map_or_else` won't work here
        // due to move semantics in both cases.
        #[allow(clippy::option_if_let_else)]
        if let Some(ratelimiter) = self.state.ratelimiter.as_ref() {
            let rx = ratelimiter.ticket(route.path());

            Ok(ResponseFuture::ratelimit(
                None,
                token_invalid,
                rx,
                self.state.timeout,
                inner,
            ))
        } else {
            Ok(ResponseFuture::new(
                token_invalid,
                time::timeout(self.state.timeout, inner),
                None,
            ))
        }
    }
}
