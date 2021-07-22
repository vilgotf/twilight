mod channel;
mod guild;
mod kind;

pub use self::{channel::WebhookChannel, guild::WebhookGuild, kind::WebhookType};

use crate::{
    id::{ApplicationId, ChannelId, GuildId, WebhookId},
    user::User,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct Webhook {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_id: Option<ApplicationId>,
    pub avatar: Option<String>,
    pub channel_id: ChannelId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guild_id: Option<GuildId>,
    pub id: WebhookId,
    #[serde(default = "WebhookType::default", rename = "type")]
    pub kind: WebhookType,
    pub name: Option<String>,
    /// Partial channel object that a webhook is following.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_channel: Option<WebhookChannel>,
    /// Partial guild object that a webhook is following.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_guild: Option<WebhookGuild>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// Url used for executing the webhook.
    ///
    /// Returned by the [`webhooks` OAuth2] flow.
    ///
    /// [`webhooks` OAuth2]: https://discord.com/developers/docs/topics/oauth2#webhooks
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<User>,
}

#[cfg(test)]
mod tests {
    use super::{
        ApplicationId, ChannelId, GuildId, User, Webhook, WebhookChannel, WebhookGuild, WebhookId,
        WebhookType,
    };
    use crate::id::UserId;
    use serde::{Deserialize, Serialize};
    use serde_test::Token;
    use static_assertions::{assert_fields, assert_impl_all};
    use std::{fmt::Debug, hash::Hash};

    assert_fields!(
        Webhook: application_id,
        avatar,
        channel_id,
        guild_id,
        id,
        kind,
        name,
        source_channel,
        source_guild,
        token,
        user
    );

    assert_impl_all!(
        Webhook: Clone,
        Debug,
        Deserialize<'static>,
        Eq,
        Hash,
        PartialEq,
        Serialize
    );

    #[test]
    fn test_webhook() {
        let value = Webhook {
            application_id: Some(ApplicationId::new(4).expect("non zero")),
            avatar: Some("avatar".to_owned()),
            channel_id: ChannelId::new(1).expect("non zero"),
            guild_id: Some(GuildId::new(2).expect("non zero")),
            id: WebhookId::new(3).expect("non zero"),
            kind: WebhookType::Incoming,
            name: Some("a webhook".to_owned()),
            source_channel: None,
            source_guild: None,
            token: Some("a token".to_owned()),
            url: None,
            user: None,
        };

        serde_test::assert_tokens(
            &value,
            &[
                Token::Struct {
                    name: "Webhook",
                    len: 8,
                },
                Token::Str("application_id"),
                Token::Some,
                Token::NewtypeStruct {
                    name: "ApplicationId",
                },
                Token::Str("4"),
                Token::Str("avatar"),
                Token::Some,
                Token::Str("avatar"),
                Token::Str("channel_id"),
                Token::NewtypeStruct { name: "ChannelId" },
                Token::Str("1"),
                Token::Str("guild_id"),
                Token::Some,
                Token::NewtypeStruct { name: "GuildId" },
                Token::Str("2"),
                Token::Str("id"),
                Token::NewtypeStruct { name: "WebhookId" },
                Token::Str("3"),
                Token::Str("type"),
                Token::U8(1),
                Token::Str("name"),
                Token::Some,
                Token::Str("a webhook"),
                Token::Str("token"),
                Token::Some,
                Token::Str("a token"),
                Token::StructEnd,
            ],
        );
    }

    #[allow(clippy::too_many_lines)]
    #[test]
    fn test_webhook_complete() {
        let value = Webhook {
            application_id: Some(ApplicationId::new(4).expect("non zero")),
            avatar: Some("avatar".to_owned()),
            channel_id: ChannelId::new(1).expect("non zero"),
            guild_id: Some(GuildId::new(2).expect("non zero")),
            id: WebhookId::new(3).expect("non zero"),
            kind: WebhookType::Incoming,
            name: Some("a webhook".to_owned()),
            source_channel: Some(WebhookChannel {
                id: ChannelId::new(4).expect("non zero"),
                name: "webhook channel".into(),
            }),
            source_guild: Some(WebhookGuild {
                icon: Some("guild icon".into()),
                id: GuildId::new(5).expect("non zero"),
                name: "webhook guild".into(),
            }),
            token: Some("a token".to_owned()),
            url: Some("https://a-url".to_owned()),
            user: Some(User {
                avatar: None,
                bot: false,
                discriminator: "0001".to_owned(),
                email: None,
                flags: None,
                id: UserId::new(2).expect("non zero"),
                locale: None,
                mfa_enabled: None,
                name: "test".to_owned(),
                premium_type: None,
                public_flags: None,
                system: None,
                verified: None,
            }),
        };

        serde_test::assert_tokens(
            &value,
            &[
                Token::Struct {
                    name: "Webhook",
                    len: 12,
                },
                Token::Str("application_id"),
                Token::Some,
                Token::NewtypeStruct {
                    name: "ApplicationId",
                },
                Token::Str("4"),
                Token::Str("avatar"),
                Token::Some,
                Token::Str("avatar"),
                Token::Str("channel_id"),
                Token::NewtypeStruct { name: "ChannelId" },
                Token::Str("1"),
                Token::Str("guild_id"),
                Token::Some,
                Token::NewtypeStruct { name: "GuildId" },
                Token::Str("2"),
                Token::Str("id"),
                Token::NewtypeStruct { name: "WebhookId" },
                Token::Str("3"),
                Token::Str("type"),
                Token::U8(1),
                Token::Str("name"),
                Token::Some,
                Token::Str("a webhook"),
                Token::Str("source_channel"),
                Token::Some,
                Token::Struct {
                    name: "WebhookChannel",
                    len: 2,
                },
                Token::Str("id"),
                Token::NewtypeStruct { name: "ChannelId" },
                Token::Str("4"),
                Token::Str("name"),
                Token::Str("webhook channel"),
                Token::StructEnd,
                Token::Str("source_guild"),
                Token::Some,
                Token::Struct {
                    name: "WebhookGuild",
                    len: 3,
                },
                Token::Str("icon"),
                Token::Some,
                Token::Str("guild icon"),
                Token::Str("id"),
                Token::NewtypeStruct { name: "GuildId" },
                Token::Str("5"),
                Token::Str("name"),
                Token::Str("webhook guild"),
                Token::StructEnd,
                Token::Str("token"),
                Token::Some,
                Token::Str("a token"),
                Token::Str("url"),
                Token::Some,
                Token::Str("https://a-url"),
                Token::Str("user"),
                Token::Some,
                Token::Struct {
                    name: "User",
                    len: 5,
                },
                Token::Str("avatar"),
                Token::None,
                Token::Str("bot"),
                Token::Bool(false),
                Token::Str("discriminator"),
                Token::Str("0001"),
                Token::Str("id"),
                Token::NewtypeStruct { name: "UserId" },
                Token::Str("2"),
                Token::Str("username"),
                Token::Str("test"),
                Token::StructEnd,
                Token::StructEnd,
            ],
        );
    }
}
