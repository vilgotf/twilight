use super::UpdateChannelPermissionConfigured;
use crate::client::Client;
use twilight_model::{
    channel::permission_overwrite::PermissionOverwriteType,
    guild::Permissions,
    id::{ChannelId, RoleId, UserId},
};

/// Update the permissions for a role or a user in a channel.
///
/// # Examples:
///
/// Create permission overrides for a role to view the channel, but not send messages:
///
/// ```no_run
/// use twilight_http::Client;
/// use twilight_model::guild::Permissions;
/// use twilight_model::id::{ChannelId, RoleId};
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::new("my token".to_owned());
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
pub struct UpdateChannelPermission<'a> {
    allow: Permissions,
    channel_id: ChannelId,
    deny: Permissions,
    http: &'a Client,
}

impl<'a> UpdateChannelPermission<'a> {
    pub(crate) const fn new(
        http: &'a Client,
        channel_id: ChannelId,
        allow: Permissions,
        deny: Permissions,
    ) -> Self {
        Self {
            allow,
            channel_id,
            deny,
            http,
        }
    }

    /// Specify this override to be for a member.
    pub const fn member(self, user_id: UserId) -> UpdateChannelPermissionConfigured<'a> {
        self.configure(PermissionOverwriteType::Member(user_id))
    }

    /// Specify this override to be for a role.
    pub const fn role(self, role_id: RoleId) -> UpdateChannelPermissionConfigured<'a> {
        self.configure(PermissionOverwriteType::Role(role_id))
    }

    const fn configure(
        self,
        target: PermissionOverwriteType,
    ) -> UpdateChannelPermissionConfigured<'a> {
        UpdateChannelPermissionConfigured::new(
            self.http,
            self.channel_id,
            self.allow,
            self.deny,
            target,
        )
    }
}
