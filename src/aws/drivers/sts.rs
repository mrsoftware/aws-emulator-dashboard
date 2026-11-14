use crate::app_state::Resource;
use anyhow::{Context, Result, bail};
use aws_config::SdkConfig;
use aws_credential_types::Credentials;

pub const LABEL: &str = "STS";

/// The session name the workbench assumes with when the caller names none.
pub const DEFAULT_SESSION_NAME: &str = "dashboard";

/// One mint the workbench made and verified. No secret material: the workbench
/// verifies a mint, it is not a keychain — the secret is shown once in the result
/// area and then dropped.
#[derive(Clone)]
pub struct MintedSession {
    pub role_name: String,
    pub session_name: String,
    pub identity_arn: String,
    pub expires: String,
    pub access_key_id: String,
}

/// The account's assumable roles. STS has no list operation, so the roles come
/// from the IAM client the dashboard already carries — the page's resource list
/// is the roles it could assume, and the row's handle is the role ARN the
/// assume call is made with.
pub async fn fetch(config: &SdkConfig) -> Result<Vec<Resource>> {
    Ok(crate::aws::drivers::iam::list_roles(config)
        .await?
        .iter()
        .map(|role| Resource {
            name: role.role_name().to_string(),
            detail: role.arn().to_string(),
        })
        .collect())
}

fn expires_text(expiration: Option<&aws_smithy_types::DateTime>) -> String {
    expiration
        .and_then(|d| d.fmt(aws_smithy_types::date_time::Format::DateTime).ok())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Assume `role_arn` as `session_name` and verify the minted credential: a second
/// STS client signed with it calls `GetCallerIdentity`, and the identity it
/// reports must be the assumed-role session the mint asked for.
///
/// The secret is handed back for the one-time reveal; it is never logged and never
/// stored beyond the call that needed it.
pub async fn assume(
    config: &SdkConfig,
    role_arn: &str,
    session_name: &str,
) -> Result<MintedSession> {
    let sts = aws_sdk_sts::Client::new(config);
    let minted = sts
        .assume_role()
        .role_arn(role_arn)
        .role_session_name(session_name)
        .send()
        .await
        .context("AssumeRole")?;
    let credentials = minted
        .credentials()
        .context("AssumeRole returned no credentials")?;
    let access_key_id = credentials.access_key_id().to_string();
    let secret = credentials.secret_access_key().to_string();
    let session_token = credentials.session_token().to_string();
    let expires = expires_text(Some(credentials.expiration()));
    let role_name = role_arn.rsplit('/').next().unwrap_or(role_arn).to_string();

    // The minted credential must speak for the session it was minted for, not for
    // some ambient identity a broken emulator would answer with instead.
    let assumed_client = aws_sdk_sts::Client::from_conf(
        aws_sdk_sts::Config::from(config)
            .to_builder()
            .credentials_provider(Credentials::new(
                access_key_id.clone(),
                secret,
                Some(session_token),
                None,
                "assumed-session",
            ))
            .build(),
    );
    let who = assumed_client
        .get_caller_identity()
        .send()
        .await
        .context("GetCallerIdentity with the minted credentials")?;
    let identity_arn = who
        .arn()
        .context("GetCallerIdentity returned no Arn")?
        .to_string();
    let expected = format!("assumed-role/{role_name}/{session_name}");
    if !identity_arn.contains(&expected) {
        bail!(
            "the minted credential reports {identity_arn}, not the assumed-role session \
             {expected} it was minted for"
        );
    }

    Ok(MintedSession {
        role_name,
        session_name: session_name.to_string(),
        identity_arn,
        expires,
        access_key_id,
    })
}
