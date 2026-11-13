use crate::app_state::Resource;
use anyhow::Result;
use aws_config::SdkConfig;
use aws_sdk_dynamodb::Client as DynamoDbClient;
use aws_sdk_s3::Client as S3Client;
use aws_sdk_sqs::Client as SqsClient;
use aws_smithy_types::date_time::Format;

pub async fn fetch_s3(config: &SdkConfig) -> Result<Vec<Resource>> {
    let client = S3Client::new(config);
    let resp = client.list_buckets().send().await?;
    Ok(resp
        .buckets
        .unwrap_or_default()
        .into_iter()
        .map(|b| Resource {
            name: b.name.unwrap_or_default(),
            detail: b
                .creation_date
                .and_then(|d| d.fmt(Format::DateTime).ok())
                .unwrap_or_default(),
        })
        .collect())
}

pub async fn fetch_dynamodb(config: &SdkConfig) -> Result<Vec<Resource>> {
    let client = DynamoDbClient::new(config);
    let mut names = Vec::new();
    let mut start = None;
    loop {
        let mut req = client.list_tables();
        if let Some(name) = start {
            req = req.exclusive_start_table_name(name);
        }
        let resp = req.send().await?;
        names.extend(resp.table_names.unwrap_or_default());
        start = resp.last_evaluated_table_name;
        if start.is_none() {
            break;
        }
    }

    let mut items = Vec::new();
    for name in names {
        let detail = match client.describe_table().table_name(&name).send().await {
            Ok(resp) => resp
                .table
                .map(|t| {
                    format!(
                        "{} · {} items",
                        t.table_status
                            .map(|s| s.as_str().to_string())
                            .unwrap_or_else(|| "UNKNOWN".to_string()),
                        t.item_count.unwrap_or(0)
                    )
                })
                .unwrap_or_default(),
            Err(_) => String::new(),
        };
        items.push(Resource { name, detail });
    }
    Ok(items)
}

pub async fn fetch_sqs(config: &SdkConfig) -> Result<Vec<Resource>> {
    let client = SqsClient::new(config);
    let mut urls = Vec::new();
    let mut token = None;
    loop {
        let mut req = client.list_queues();
        if let Some(t) = token {
            req = req.next_token(t);
        }
        let resp = req.send().await?;
        urls.extend(resp.queue_urls.unwrap_or_default());
        token = resp.next_token;
        if token.is_none() {
            break;
        }
    }
    Ok(urls
        .into_iter()
        .map(|url| Resource {
            name: url.rsplit('/').next().unwrap_or(&url).to_string(),
            detail: url,
        })
        .collect())
}
