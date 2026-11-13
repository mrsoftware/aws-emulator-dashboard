use crate::app_state::Resource;
use anyhow::{Context, Result};
use aws_config::SdkConfig;
use aws_sdk_iam::Client;
use aws_smithy_types::date_time::Format;

pub const LABEL: &str = "IAM";

const DEFAULT_TRUST_POLICY: &str = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"Service":"ec2.amazonaws.com"},"Action":"sts:AssumeRole"}]}"#;

fn date(value: Option<&aws_smithy_types::DateTime>) -> String {
    value
        .and_then(|d| d.fmt(Format::DateTime).ok())
        .unwrap_or_else(|| "unknown".to_string())
}

async fn all_users(config: &SdkConfig) -> Result<Vec<aws_sdk_iam::types::User>> {
    let client = Client::new(config);
    let mut users = Vec::new();
    let mut marker: Option<String> = None;
    loop {
        let mut req = client.list_users().max_items(1000);
        if let Some(m) = marker.clone() {
            req = req.marker(m);
        }
        let resp = req.send().await.context("ListUsers")?;
        users.extend_from_slice(resp.users());
        marker = resp.marker().map(str::to_string);
        if !resp.is_truncated() || marker.is_none() {
            break;
        }
    }
    Ok(users)
}

pub async fn fetch(config: &SdkConfig) -> Result<Vec<Resource>> {
    Ok(all_users(config)
        .await?
        .iter()
        .map(|u| Resource {
            name: u.user_name().to_string(),
            detail: format!(
                "created {} · path {}",
                date(Some(u.create_date())),
                u.path()
            ),
        })
        .collect())
}

pub async fn fetch_roles(config: &SdkConfig) -> Result<Vec<Resource>> {
    let client = Client::new(config);
    let mut roles = Vec::new();
    let mut marker: Option<String> = None;
    loop {
        let mut req = client.list_roles().max_items(1000);
        if let Some(m) = marker.clone() {
            req = req.marker(m);
        }
        let resp = req.send().await.context("ListRoles")?;
        for r in resp.roles() {
            roles.push(Resource {
                name: r.role_name().to_string(),
                detail: format!(
                    "created {} · path {}",
                    date(Some(r.create_date())),
                    r.path()
                ),
            });
        }
        marker = resp.marker().map(str::to_string);
        if !resp.is_truncated() || marker.is_none() {
            break;
        }
    }
    Ok(roles)
}

/// CreateUser raises EntityAlreadyExistsException on a name clash; the create path
/// pre-checks through `fetch`, so this is the live tests' own probe.
#[cfg(test)]
pub async fn user_exists(config: &SdkConfig, name: &str) -> Result<bool> {
    let users = all_users(config).await?;
    Ok(users.iter().any(|u| u.user_name() == name))
}

pub async fn create_user(config: &SdkConfig, name: &str) -> Result<()> {
    let client = Client::new(config);
    client
        .create_user()
        .user_name(name)
        .send()
        .await
        .context("CreateUser")?;
    Ok(())
}

pub async fn delete_user(config: &SdkConfig, name: &str) -> Result<()> {
    let client = Client::new(config);
    client
        .delete_user()
        .user_name(name)
        .send()
        .await
        .context("DeleteUser")?;
    Ok(())
}

pub async fn list_access_keys(config: &SdkConfig, user: &str) -> Result<Vec<Resource>> {
    let client = Client::new(config);
    let resp = client
        .list_access_keys()
        .user_name(user)
        .send()
        .await
        .context("ListAccessKeys")?;
    Ok(resp
        .access_key_metadata()
        .iter()
        .map(|k| Resource {
            name: k.access_key_id().unwrap_or_default().to_string(),
            detail: format!(
                "{} · created {}",
                k.status().map(|s| s.as_str()).unwrap_or("Unknown"),
                date(k.create_date())
            ),
        })
        .collect())
}

/// Returns (access key id, secret access key). The secret is handed to the
/// caller exactly once and must never be logged or persisted.
pub async fn create_access_key(config: &SdkConfig, user: &str) -> Result<(String, String)> {
    let client = Client::new(config);
    let resp = client
        .create_access_key()
        .user_name(user)
        .send()
        .await
        .context("CreateAccessKey")?;
    let key = resp
        .access_key()
        .context("CreateAccessKey returned no access key")?;
    Ok((
        key.access_key_id().to_string(),
        key.secret_access_key().to_string(),
    ))
}

pub async fn delete_access_key(config: &SdkConfig, user: &str, key_id: &str) -> Result<()> {
    let client = Client::new(config);
    client
        .delete_access_key()
        .user_name(user)
        .access_key_id(key_id)
        .send()
        .await
        .context("DeleteAccessKey")?;
    Ok(())
}

pub async fn create_role(config: &SdkConfig, name: &str) -> Result<()> {
    let client = Client::new(config);
    client
        .create_role()
        .role_name(name)
        .assume_role_policy_document(DEFAULT_TRUST_POLICY)
        .send()
        .await
        .context("CreateRole")?;
    Ok(())
}

pub async fn delete_role(config: &SdkConfig, name: &str) -> Result<()> {
    let client = Client::new(config);
    client
        .delete_role()
        .role_name(name)
        .send()
        .await
        .context("DeleteRole")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aws::client::EmulatorConnection;

    fn run_id() -> String {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_micros()
            .to_string()
    }

    #[tokio::test]
    #[ignore = "requires a running emulator on 127.0.0.1:9297; creates and deletes an IAM user, access key and role"]
    async fn live_iam_lifecycle() {
        let conn = EmulatorConnection::from_port(9297);
        let config = conn.build_aws_config().await.expect("aws config");

        let id = run_id();
        let user = format!("dashboard-iam-user-{id}");
        let role = format!("dashboard-iam-role-{id}");

        create_user(&config, &user).await.expect("create user");
        assert!(
            user_exists(&config, &user).await.expect("user exists"),
            "created user not found via user_exists"
        );
        let listed = fetch(&config).await.expect("list users");
        assert!(
            listed.iter().any(|r| r.name == user),
            "created user missing from ListUsers"
        );

        // Tracks a key that still needs deleting so cleanup never leaves residue.
        let mut live_key: Option<String> = None;
        let outcome = async {
            let (key_id, secret) = create_access_key(&config, &user).await?;
            live_key = Some(key_id.clone());
            assert!(!key_id.is_empty(), "empty access key id");
            assert!(!secret.is_empty(), "empty secret access key"); // secret never printed

            let keys = list_access_keys(&config, &user).await?;
            assert!(
                keys.iter().any(|k| k.name == key_id),
                "created access key missing from ListAccessKeys"
            );

            delete_access_key(&config, &user, &key_id).await?;
            live_key = None;
            let keys = list_access_keys(&config, &user).await?;
            assert!(
                !keys.iter().any(|k| k.name == key_id),
                "access key survived delete"
            );

            create_role(&config, &role).await?;
            let roles = fetch_roles(&config).await?;
            assert!(
                roles.iter().any(|r| r.name == role),
                "created role missing from ListRoles"
            );

            delete_role(&config, &role).await?;
            let roles = fetch_roles(&config).await?;
            assert!(
                !roles.iter().any(|r| r.name == role),
                "role survived delete"
            );

            Ok::<_, anyhow::Error>(())
        }
        .await;

        // Deletions are not idempotent here, so cleanup is best-effort and runs
        // unconditionally: clear any live access key first (a user still holding
        // one cannot be deleted), then the role, then the user.
        if let Some(key_id) = &live_key {
            let _ = delete_access_key(&config, &user, key_id).await;
        }
        let _ = delete_role(&config, &role).await;
        let _ = delete_user(&config, &user).await;

        outcome.expect("iam lifecycle");

        assert!(
            !user_exists(&config, &user)
                .await
                .expect("user exists after cleanup"),
            "user still present after cleanup"
        );
    }
}
