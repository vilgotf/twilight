use super::ExecuteWebhookAndWait;
use crate::{
    client::Client,
    error::Error,
    request::{Form, Request},
    response::{marker::EmptyBody, ResponseFuture},
    routing::Route,
};
use serde::Serialize;
use twilight_model::{
    channel::{embed::Embed, message::AllowedMentions},
    id::WebhookId,
};

#[derive(Serialize)]
pub(crate) struct ExecuteWebhookFields<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    avatar_url: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    embeds: Option<&'a [Embed]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload_json: Option<&'a [u8]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tts: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) allowed_mentions: Option<AllowedMentions>,
}

/// Execute a webhook, sending a message to its channel.
///
/// You can only specify one of [`content`], [`embeds`], or [`files`].
///
/// # Examples
///
/// ```no_run
/// use twilight_http::Client;
/// use twilight_model::id::WebhookId;
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::new("my token".to_owned());
/// let id = WebhookId::new(432).expect("non zero");
///
/// client
///     .execute_webhook(id, "webhook token")
///     .content("Pinkie...")
///     .exec()
///     .await?;
/// # Ok(()) }
/// ```
///
/// [`content`]: Self::content
/// [`embeds`]: Self::embeds
/// [`files`]: Self::files
pub struct ExecuteWebhook<'a> {
    pub(crate) fields: ExecuteWebhookFields<'a>,
    files: &'a [(&'a str, &'a [u8])],
    pub(super) http: &'a Client,
    token: &'a str,
    webhook_id: WebhookId,
}

impl<'a> ExecuteWebhook<'a> {
    pub(crate) const fn new(http: &'a Client, webhook_id: WebhookId, token: &'a str) -> Self {
        Self {
            fields: ExecuteWebhookFields {
                avatar_url: None,
                content: None,
                embeds: None,
                payload_json: None,
                tts: None,
                username: None,
                allowed_mentions: None,
            },
            files: &[],
            http,
            token,
            webhook_id,
        }
    }

    /// Specify the [`AllowedMentions`] for the webhook message.
    pub fn allowed_mentions(mut self, allowed_mentions: AllowedMentions) -> Self {
        self.fields.allowed_mentions.replace(allowed_mentions);

        self
    }

    /// The URL of the avatar of the webhook.
    pub const fn avatar_url(mut self, avatar_url: &'a str) -> Self {
        self.fields.avatar_url = Some(avatar_url);

        self
    }

    /// The content of the webook's message.
    ///
    /// Up to 2000 UTF-16 codepoints, same as a message.
    pub const fn content(mut self, content: &'a str) -> Self {
        self.fields.content = Some(content);

        self
    }

    /// Set the list of embeds of the webhook's message.
    pub const fn embeds(mut self, embeds: &'a [Embed]) -> Self {
        self.fields.embeds = Some(embeds);

        self
    }

    /// Attach multiple files to the webhook.
    pub const fn files(mut self, files: &'a [(&'a str, &'a [u8])]) -> Self {
        self.files = files;

        self
    }

    /// JSON encoded body of any additional request fields.
    ///
    /// If this method is called, all other fields are ignored, except for
    /// [`file`]. See [Discord Docs/Create Message].
    ///
    /// # Examples
    ///
    /// Without [`payload_json`]:
    ///
    /// ```no_run
    /// use twilight_embed_builder::EmbedBuilder;
    /// # use twilight_http::Client;
    /// use twilight_model::id::{MessageId, WebhookId};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("token".to_owned());
    /// let message = client.execute_webhook(WebhookId::new(1).expect("non zero"), "token here")
    ///     .content("some content")
    ///     .embeds(&[EmbedBuilder::new().title("title").build()?])
    ///     .wait()
    ///     .exec()
    ///     .await?
    ///     .model()
    ///     .await?;
    ///
    /// assert_eq!(message.content, "some content");
    /// # Ok(()) }
    /// ```
    ///
    /// With [`payload_json`]:
    ///
    /// ```no_run
    /// # use twilight_http::Client;
    /// use twilight_model::id::{MessageId, WebhookId};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("token".to_owned());
    /// let message = client.execute_webhook(WebhookId::new(1).expect("non zero"), "token here")
    ///     .content("some content")
    ///     .payload_json(br#"{ "content": "other content", "embeds": [ { "title": "title" } ] }"#)
    ///     .wait()
    ///     .exec()
    ///     .await?
    ///     .model()
    ///     .await?;
    ///
    /// assert_eq!(message.content, "other content");
    /// # Ok(()) }
    /// ```
    ///
    /// [`payload_json`]: Self::payload_json
    /// [Discord Docs/Create Message]: https://discord.com/developers/docs/resources/channel#create-message-params
    pub const fn payload_json(mut self, payload_json: &'a [u8]) -> Self {
        self.fields.payload_json = Some(payload_json);

        self
    }

    /// Specify true if the message is TTS.
    pub const fn tts(mut self, tts: bool) -> Self {
        self.fields.tts = Some(tts);

        self
    }

    /// Specify the username of the webhook's message.
    pub const fn username(mut self, username: &'a str) -> Self {
        self.fields.username = Some(username);

        self
    }

    /// Wait for the message to send before sending a response. See
    /// [Discord Docs/Execute Webhook].
    ///
    /// Using this will result in receiving the created message.
    ///
    /// [Discord Docs/Execute Webhook]: https://discord.com/developers/docs/resources/webhook#execute-webhook-querystring-params
    #[allow(clippy::missing_const_for_fn)]
    pub fn wait(self) -> ExecuteWebhookAndWait<'a> {
        ExecuteWebhookAndWait::new(self)
    }

    // `self` needs to be consumed and the client returned due to parameters
    // being consumed in request construction.
    pub(super) fn request(&self, wait: bool) -> Result<Request<'a>, Error> {
        let mut request = Request::builder(Route::ExecuteWebhook {
            token: self.token,
            wait: Some(wait),
            webhook_id: self.webhook_id.0.get(),
        });

        // Webhook executions don't need the authorization token, only the
        // webhook token.
        request = request.use_authorization_token(false);

        if !self.files.is_empty() || self.fields.payload_json.is_some() {
            let mut form = Form::new();

            for (index, (name, file)) in self.files.iter().enumerate() {
                form.file(format!("{}", index).as_bytes(), name.as_bytes(), file);
            }

            if let Some(payload_json) = &self.fields.payload_json {
                form.payload_json(&payload_json);
            } else {
                let body = crate::json::to_vec(&self.fields).map_err(Error::json)?;

                form.payload_json(&body);
            }

            request = request.form(form);
        } else {
            request = request.json(&self.fields)?;
        }

        Ok(request.build())
    }

    /// Execute the request, returning a future resolving to a [`Response`].
    ///
    /// [`Response`]: crate::response::Response
    pub fn exec(self) -> ResponseFuture<EmptyBody> {
        match self.request(false) {
            Ok(request) => self.http.request(request),
            Err(source) => ResponseFuture::error(source),
        }
    }
}
