use crate::{
    client::Client,
    request::{self, AuditLogReason, AuditLogReasonError, Request},
    response::ResponseFuture,
    routing::Route,
};
use serde::Serialize;
use twilight_model::{
    guild::{Permissions, Role},
    id::GuildId,
};

#[derive(Serialize)]
struct CreateRoleFields<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    color: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hoist: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mentionable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    permissions: Option<Permissions>,
}

/// Create a role in a guild.
///
/// # Examples
///
/// ```no_run
/// use twilight_http::Client;
/// use twilight_model::id::GuildId;
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::new("my token".to_owned());
/// let guild_id = GuildId::new(234).expect("non zero");
///
/// client.create_role(guild_id)
///     .color(0xd90083)
///     .name("Bright Pink")
///     .exec()
///     .await?;
/// # Ok(()) }
/// ```
pub struct CreateRole<'a> {
    fields: CreateRoleFields<'a>,
    guild_id: GuildId,
    http: &'a Client,
    reason: Option<&'a str>,
}

impl<'a> CreateRole<'a> {
    pub(crate) const fn new(http: &'a Client, guild_id: GuildId) -> Self {
        Self {
            fields: CreateRoleFields {
                color: None,
                hoist: None,
                mentionable: None,
                name: None,
                permissions: None,
            },
            guild_id,
            http,
            reason: None,
        }
    }

    /// Set the color of the role.
    pub const fn color(mut self, color: u32) -> Self {
        self.fields.color = Some(color);

        self
    }

    /// If true, display the role in the members list.
    pub const fn hoist(mut self, hoist: bool) -> Self {
        self.fields.hoist = Some(hoist);

        self
    }

    /// If true, the role can be @mentioned (pinged) in chat.
    pub const fn mentionable(mut self, mentionable: bool) -> Self {
        self.fields.mentionable = Some(mentionable);

        self
    }

    /// Set the name of the role.
    ///
    /// If none is specified, Discord sets this to `New Role`.
    pub const fn name(mut self, name: &'a str) -> Self {
        self.fields.name = Some(name);

        self
    }

    /// Set the allowed permissions of this role.
    pub const fn permissions(mut self, permissions: Permissions) -> Self {
        self.fields.permissions = Some(permissions);

        self
    }

    /// Execute the request, returning a future resolving to a [`Response`].
    ///
    /// [`Response`]: crate::response::Response
    pub fn exec(self) -> ResponseFuture<Role> {
        let mut request = Request::builder(Route::CreateRole {
            guild_id: self.guild_id.0.get(),
        });

        request = match request.json(&self.fields) {
            Ok(request) => request,
            Err(source) => return ResponseFuture::error(source),
        };

        if let Some(reason) = &self.reason {
            let header = match request::audit_header(reason) {
                Ok(header) => header,
                Err(source) => return ResponseFuture::error(source),
            };

            request = request.headers(header);
        }

        self.http.request(request.build())
    }
}

impl<'a> AuditLogReason<'a> for CreateRole<'a> {
    fn reason(mut self, reason: &'a str) -> Result<Self, AuditLogReasonError> {
        self.reason.replace(AuditLogReasonError::validate(reason)?);

        Ok(self)
    }
}
