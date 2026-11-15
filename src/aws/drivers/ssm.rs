use crate::app_state::Resource;
use anyhow::{Context, Result};
use aws_config::SdkConfig;
use aws_sdk_ssm::types::ParameterType;

pub const LABEL: &str = "SSM";

/// The type a create dialog gets when the caller leaves the field empty.
pub const DEFAULT_PARAMETER_TYPE: &str = "String";

fn parse_parameter_type(value: &str) -> Result<ParameterType> {
    match value {
        "String" => Ok(ParameterType::String),
        "StringList" => Ok(ParameterType::StringList),
        "SecureString" => Ok(ParameterType::SecureString),
        other => Err(anyhow::anyhow!(
            "{other} is not a parameter type; String, StringList or SecureString"
        )),
    }
}

fn modified_text(date: Option<&aws_smithy_types::DateTime>) -> String {
    date.and_then(|d| d.fmt(aws_smithy_types::date_time::Format::DateTime).ok())
        .unwrap_or_else(|| "unknown".to_string())
}

/// The account's parameters. The page manages Parameter Store and says so: the
/// rest of SSM is not served by the emulator.
pub async fn fetch(config: &SdkConfig) -> Result<Vec<Resource>> {
    let client = aws_sdk_ssm::Client::new(config);
    let mut parameters = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let mut req = client.describe_parameters().max_results(50);
        if let Some(t) = token.clone() {
            req = req.next_token(t);
        }
        let resp = req.send().await.context("DescribeParameters")?;
        for p in resp.parameters() {
            parameters.push(Resource {
                name: p.name().unwrap_or_default().to_string(),
                detail: format!(
                    "{} · modified {}",
                    match p.r#type() {
                        Some(t) => t.as_str(),
                        None => "unknown type",
                    },
                    modified_text(p.last_modified_date()),
                ),
            });
        }
        token = resp.next_token().map(str::to_string);
        if token.is_none() {
            break;
        }
    }
    Ok(parameters)
}

/// Put a parameter. `Overwrite` stays unset, so a name the account already holds
/// is refused by the service; the create path pre-checks the listing first.
pub async fn create(config: &SdkConfig, name: &str, value: &str, type_str: &str) -> Result<()> {
    let parsed = parse_parameter_type(type_str)?;
    aws_sdk_ssm::Client::new(config)
        .put_parameter()
        .name(name)
        .value(value)
        .r#type(parsed)
        .send()
        .await
        .context("PutParameter")?;
    Ok(())
}

/// The parameter's value, decrypted when it is a `SecureString`. The emulator's
/// KMS gate decides whether the caller may read a sealed value back; its refusal
/// is the error the caller sees, never a faked plaintext.
pub async fn reveal(config: &SdkConfig, name: &str) -> Result<(String, String)> {
    let parameter = aws_sdk_ssm::Client::new(config)
        .get_parameter()
        .name(name)
        .with_decryption(true)
        .send()
        .await
        .context("GetParameter")?
        .parameter()
        .context("GetParameter returned no parameter")?
        .clone();
    let value = parameter.value().context("parameter has no value")?;
    Ok((
        parameter
            .r#type()
            .map(|t| t.as_str().to_string())
            .unwrap_or_else(|| "unknown type".to_string()),
        value.to_string(),
    ))
}

/// The parameter's version history, newest last as the API serves it.
pub async fn history(config: &SdkConfig, name: &str) -> Result<Vec<Resource>> {
    let client = aws_sdk_ssm::Client::new(config);
    let mut versions = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let mut req = client.get_parameter_history().name(name).max_results(50);
        if let Some(t) = token.clone() {
            req = req.next_token(t);
        }
        let resp = req.send().await.context("GetParameterHistory")?;
        for v in resp.parameters() {
            let label = v
                .labels()
                .first()
                .map(|l| format!(" · {l}"))
                .unwrap_or_default();
            versions.push(Resource {
                name: format!("version {}{label}", v.version()),
                detail: format!(
                    "{} · modified {}",
                    match v.r#type() {
                        Some(t) => t.as_str(),
                        None => "unknown type",
                    },
                    modified_text(v.last_modified_date()),
                ),
            });
        }
        token = resp.next_token().map(str::to_string);
        if token.is_none() {
            break;
        }
    }
    Ok(versions)
}

pub async fn delete(config: &SdkConfig, name: &str) -> Result<()> {
    aws_sdk_ssm::Client::new(config)
        .delete_parameter()
        .name(name)
        .send()
        .await
        .context("DeleteParameter")?;
    Ok(())
}
