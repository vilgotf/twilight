use crate::{
    client::Client,
    error::Error,
    request::{
        application::{InteractionError, InteractionErrorType},
        validate, Request, RequestBuilder,
    },
    response::ResponseFuture,
    routing::Route,
};
use serde::{Serialize, Serializer};
use std::num::NonZeroU64;
use twilight_model::{
    application::command::permissions::CommandPermissions,
    id::{ApplicationId, CommandId, GuildId},
};

#[derive(Serialize)]
struct GuildCommandPermission<'a> {
    id: &'a CommandId,
    permissions: &'a CommandPermissions,
}

struct PermissionListSerializer<'a> {
    inner: &'a [(CommandId, CommandPermissions)],
}

impl Serialize for PermissionListSerializer<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(
            self.inner
                .iter()
                .map(|(id, permissions)| GuildCommandPermission { id, permissions }),
        )
    }
}

/// Update command permissions for all commands in a guild.
///
/// This overwrites the command permissions so the full set of permissions
/// have to be sent every time.
#[derive(Debug)]
pub struct SetCommandPermissions<'a> {
    application_id: ApplicationId,
    guild_id: GuildId,
    http: &'a Client,
    permissions: &'a [(CommandId, CommandPermissions)],
}

impl<'a> SetCommandPermissions<'a> {
    pub(crate) fn new(
        http: &'a Client,
        application_id: ApplicationId,
        guild_id: GuildId,
        permissions: &'a [(CommandId, CommandPermissions)],
    ) -> Result<Self, InteractionError> {
        let mut sorted_permissions = [(CommandId(NonZeroU64::new(u64::MAX).expect("non zero")), 0);
            InteractionError::GUILD_COMMAND_LIMIT];

        'outer: for (permission_id, _) in permissions {
            for (ref mut sorted_id, ref mut count) in &mut sorted_permissions {
                if *sorted_id == *permission_id {
                    *count += 1;

                    if !validate::guild_command_permissions(*count) {
                        return Err(InteractionError {
                            kind: InteractionErrorType::TooManyCommandPermissions,
                        });
                    }

                    continue 'outer;
                } else if sorted_id.0 == NonZeroU64::new(u64::MAX).expect("non zero") {
                    *count += 1;
                    *sorted_id = *permission_id;

                    continue 'outer;
                }
            }

            // We've run out of space in the sorted permissions, which means the
            // user provided too many commands.
            return Err(InteractionError {
                kind: InteractionErrorType::TooManyCommands,
            });
        }

        Ok(Self {
            application_id,
            guild_id,
            http,
            permissions,
        })
    }

    fn request(&self) -> Result<Request<'a>, Error> {
        Request::builder(Route::SetCommandPermissions {
            application_id: self.application_id.0.get(),
            guild_id: self.guild_id.0.get(),
        })
        .json(&PermissionListSerializer {
            inner: self.permissions,
        })
        .map(RequestBuilder::build)
    }

    /// Execute the request, returning a future resolving to a [`Response`].
    ///
    /// [`Response`]: crate::response::Response
    pub fn exec(self) -> ResponseFuture<CommandPermissions> {
        match self.request() {
            Ok(request) => self.http.request(request),
            Err(source) => ResponseFuture::error(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        super::{InteractionError, InteractionErrorType},
        SetCommandPermissions,
    };
    use crate::Client;
    use std::{iter, num::NonZeroU64};
    use twilight_model::{
        application::command::permissions::{CommandPermissions, CommandPermissionsType},
        id::{ApplicationId, CommandId, GuildId, RoleId},
    };

    // SAFETY: never zero
    const APPLICATION_ID: ApplicationId = ApplicationId(unsafe { NonZeroU64::new_unchecked(1) });
    // SAFETY: never zero
    const GUILD_ID: GuildId = GuildId(unsafe { NonZeroU64::new_unchecked(2) });

    fn command_permissions(id: CommandId) -> impl Iterator<Item = (CommandId, CommandPermissions)> {
        iter::repeat((
            id,
            CommandPermissions {
                id: CommandPermissionsType::Role(RoleId(NonZeroU64::new(4).expect("non zero"))),
                permission: true,
            },
        ))
    }

    #[test]
    fn test_correct_validation() {
        let http = Client::new("token".to_owned());
        let command_permissions =
            command_permissions(CommandId(NonZeroU64::new(1).expect("non zero")))
                .take(4)
                .collect::<Vec<_>>();

        let request =
            SetCommandPermissions::new(&http, APPLICATION_ID, GUILD_ID, &command_permissions);

        assert!(request.is_ok());
    }

    #[test]
    fn test_incorrect_validation() {
        let http = Client::new("token".to_owned());
        let command_permissions =
            command_permissions(CommandId(NonZeroU64::new(2).expect("non zero")))
                .take(InteractionError::GUILD_COMMAND_PERMISSION_LIMIT + 1)
                .collect::<Vec<_>>();

        let request =
            SetCommandPermissions::new(&http, APPLICATION_ID, GUILD_ID, &command_permissions);
        assert!(matches!(
            request.unwrap_err().kind(),
            InteractionErrorType::TooManyCommandPermissions
        ));
    }

    #[test]
    fn test_limits() {
        const SIZE: usize = InteractionError::GUILD_COMMAND_LIMIT;

        let http = Client::new("token".to_owned());
        let command_permissions = (1..=SIZE)
            .flat_map(|id| {
                command_permissions(CommandId(NonZeroU64::new(id as u64).expect("non zero")))
                    .take(InteractionError::GUILD_COMMAND_PERMISSION_LIMIT)
            })
            .collect::<Vec<_>>();

        assert!(
            SetCommandPermissions::new(&http, APPLICATION_ID, GUILD_ID, &command_permissions)
                .is_ok()
        );
    }

    #[test]
    fn test_command_count_over_limit() {
        const SIZE: usize = 101;

        let http = Client::new("token".to_owned());
        let command_permissions = (1..=SIZE)
            .flat_map(|id| {
                command_permissions(CommandId(NonZeroU64::new(id as u64).expect("non zero")))
                    .take(3)
            })
            .collect::<Vec<_>>();

        let request =
            SetCommandPermissions::new(&http, APPLICATION_ID, GUILD_ID, &command_permissions);
        assert!(matches!(
            request.unwrap_err().kind(),
            InteractionErrorType::TooManyCommands
        ));
    }

    #[test]
    fn test_no_permissions() {
        let http = Client::new("token".to_owned());

        assert!(SetCommandPermissions::new(&http, APPLICATION_ID, GUILD_ID, &[]).is_ok());
    }
}
