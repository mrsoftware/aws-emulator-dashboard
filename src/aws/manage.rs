use anyhow::{Context, Result, bail};
use aws_config::SdkConfig;
use aws_sdk_dynamodb::types::{
    AttributeDefinition, AttributeValue, BillingMode, KeySchemaElement, KeyType,
    ScalarAttributeType,
};
use serde_json::{Map, Value};
use std::path::Path;

#[derive(Clone)]
pub struct ObjectInfo {
    pub key: String,
    pub size: i64,
    pub modified: String,
}

#[derive(Clone)]
pub struct MessageInfo {
    pub id: String,
    pub body: String,
    pub receipt: String,
}

pub async fn create_bucket(config: &SdkConfig, name: &str) -> Result<()> {
    aws_sdk_s3::Client::new(config)
        .create_bucket()
        .bucket(name)
        .send()
        .await
        .context("CreateBucket")?;
    Ok(())
}

pub async fn delete_bucket(config: &SdkConfig, name: &str) -> Result<()> {
    aws_sdk_s3::Client::new(config)
        .delete_bucket()
        .bucket(name)
        .send()
        .await
        .context("DeleteBucket")?;
    Ok(())
}

pub async fn list_objects(config: &SdkConfig, bucket: &str) -> Result<Vec<ObjectInfo>> {
    let client = aws_sdk_s3::Client::new(config);
    let mut objects = Vec::new();
    let mut token = None;
    loop {
        let mut req = client.list_objects_v2().bucket(bucket);
        if let Some(t) = token {
            req = req.continuation_token(t);
        }
        let resp = req.send().await.context("ListObjectsV2")?;
        for o in resp.contents() {
            objects.push(ObjectInfo {
                key: o.key().unwrap_or_default().to_string(),
                size: o.size().unwrap_or(0),
                modified: o
                    .last_modified()
                    .map(|t| {
                        t.fmt(aws_smithy_types::date_time::Format::DateTime)
                            .unwrap_or_default()
                    })
                    .unwrap_or_default(),
            });
        }
        match resp.next_continuation_token() {
            Some(t) => token = Some(t.to_string()),
            None => break,
        }
    }
    Ok(objects)
}

pub async fn put_object(config: &SdkConfig, bucket: &str, key: &str, path: &Path) -> Result<()> {
    let body = aws_sdk_s3::primitives::ByteStream::from_path(path)
        .await
        .context("reading upload file")?;
    aws_sdk_s3::Client::new(config)
        .put_object()
        .bucket(bucket)
        .key(key)
        .body(body)
        .send()
        .await
        .context("PutObject")?;
    Ok(())
}

pub async fn get_object(config: &SdkConfig, bucket: &str, key: &str, dest: &Path) -> Result<()> {
    let resp = aws_sdk_s3::Client::new(config)
        .get_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .context("GetObject")?;
    let bytes = resp.body.collect().await.context("reading object body")?;
    std::fs::write(dest, bytes.into_bytes()).context("writing download file")?;
    Ok(())
}

pub async fn delete_object(config: &SdkConfig, bucket: &str, key: &str) -> Result<()> {
    aws_sdk_s3::Client::new(config)
        .delete_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .context("DeleteObject")?;
    Ok(())
}

pub async fn create_table(
    config: &SdkConfig,
    name: &str,
    partition_key: &str,
    sort_key: Option<&str>,
) -> Result<()> {
    let client = aws_sdk_dynamodb::Client::new(config);
    let mut req = client
        .create_table()
        .table_name(name)
        .attribute_definitions(
            AttributeDefinition::builder()
                .attribute_name(partition_key)
                .attribute_type(ScalarAttributeType::S)
                .build()?,
        )
        .key_schema(
            KeySchemaElement::builder()
                .attribute_name(partition_key)
                .key_type(KeyType::Hash)
                .build()?,
        )
        .billing_mode(BillingMode::PayPerRequest);
    if let Some(sk) = sort_key {
        req = req
            .attribute_definitions(
                AttributeDefinition::builder()
                    .attribute_name(sk)
                    .attribute_type(ScalarAttributeType::S)
                    .build()?,
            )
            .key_schema(
                KeySchemaElement::builder()
                    .attribute_name(sk)
                    .key_type(KeyType::Range)
                    .build()?,
            );
    }
    req.send().await.context("CreateTable")?;
    Ok(())
}

pub async fn delete_table(config: &SdkConfig, name: &str) -> Result<()> {
    aws_sdk_dynamodb::Client::new(config)
        .delete_table()
        .table_name(name)
        .send()
        .await
        .context("DeleteTable")?;
    Ok(())
}

pub async fn describe_key_schema(config: &SdkConfig, name: &str) -> Result<Vec<(String, String)>> {
    let resp = aws_sdk_dynamodb::Client::new(config)
        .describe_table()
        .table_name(name)
        .send()
        .await
        .context("DescribeTable")?;
    let schema = resp
        .table()
        .map(|t| {
            t.key_schema()
                .iter()
                .map(|k| {
                    (
                        k.attribute_name().to_string(),
                        k.key_type().as_str().to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(schema)
}

pub async fn scan_items(config: &SdkConfig, table: &str, limit: i32) -> Result<Vec<String>> {
    let resp = aws_sdk_dynamodb::Client::new(config)
        .scan()
        .table_name(table)
        .limit(limit)
        .send()
        .await
        .context("Scan")?;
    let items = resp
        .items()
        .iter()
        .map(|item| {
            let map: Map<String, Value> = item
                .iter()
                .map(|(k, v)| (k.clone(), attr_to_json(v)))
                .collect();
            Value::Object(map).to_string()
        })
        .collect();
    Ok(items)
}

pub async fn put_item(config: &SdkConfig, table: &str, json: &str) -> Result<()> {
    let item = json_object_to_item(json)?;
    aws_sdk_dynamodb::Client::new(config)
        .put_item()
        .table_name(table)
        .set_item(Some(item))
        .send()
        .await
        .context("PutItem")?;
    Ok(())
}

pub async fn delete_item(config: &SdkConfig, table: &str, key_json: &str) -> Result<()> {
    let key = json_object_to_item(key_json)?;
    aws_sdk_dynamodb::Client::new(config)
        .delete_item()
        .table_name(table)
        .set_key(Some(key))
        .send()
        .await
        .context("DeleteItem")?;
    Ok(())
}

pub async fn create_queue(config: &SdkConfig, name: &str) -> Result<()> {
    aws_sdk_sqs::Client::new(config)
        .create_queue()
        .queue_name(name)
        .send()
        .await
        .context("CreateQueue")?;
    Ok(())
}

// The emulator returns success for duplicate CreateBucket/CreateTable/CreateQueue,
// so the dashboard checks existence itself before accepting a create.
pub async fn bucket_exists(config: &SdkConfig, name: &str) -> Result<bool> {
    match aws_sdk_s3::Client::new(config)
        .head_bucket()
        .bucket(name)
        .send()
        .await
    {
        Ok(_) => Ok(true),
        Err(e) if e.as_service_error().is_some_and(|s| s.is_not_found()) => Ok(false),
        Err(e) => Err(anyhow::Error::new(e).context("HeadBucket")),
    }
}

pub async fn table_exists(config: &SdkConfig, name: &str) -> Result<bool> {
    match aws_sdk_dynamodb::Client::new(config)
        .describe_table()
        .table_name(name)
        .send()
        .await
    {
        Ok(_) => Ok(true),
        Err(e)
            if e.as_service_error()
                .is_some_and(|s| s.is_resource_not_found_exception()) =>
        {
            Ok(false)
        }
        Err(e) => Err(anyhow::Error::new(e).context("DescribeTable")),
    }
}

pub async fn queue_exists(config: &SdkConfig, name: &str) -> Result<bool> {
    match aws_sdk_sqs::Client::new(config)
        .get_queue_url()
        .queue_name(name)
        .send()
        .await
    {
        Ok(_) => Ok(true),
        Err(e)
            if e.as_service_error()
                .is_some_and(|s| s.is_queue_does_not_exist()) =>
        {
            Ok(false)
        }
        Err(e) => Err(anyhow::Error::new(e).context("GetQueueUrl")),
    }
}

pub async fn delete_queue(config: &SdkConfig, url: &str) -> Result<()> {
    aws_sdk_sqs::Client::new(config)
        .delete_queue()
        .queue_url(url)
        .send()
        .await
        .context("DeleteQueue")?;
    Ok(())
}

pub async fn send_message(config: &SdkConfig, url: &str, body: &str) -> Result<()> {
    aws_sdk_sqs::Client::new(config)
        .send_message()
        .queue_url(url)
        .message_body(body)
        .send()
        .await
        .context("SendMessage")?;
    Ok(())
}

pub async fn receive_messages(config: &SdkConfig, url: &str) -> Result<Vec<MessageInfo>> {
    let resp = aws_sdk_sqs::Client::new(config)
        .receive_message()
        .queue_url(url)
        .max_number_of_messages(10)
        .send()
        .await
        .context("ReceiveMessage")?;
    let messages = resp
        .messages()
        .iter()
        .map(|m| MessageInfo {
            id: m.message_id().unwrap_or_default().to_string(),
            body: m.body().unwrap_or_default().to_string(),
            receipt: m.receipt_handle().unwrap_or_default().to_string(),
        })
        .collect();
    Ok(messages)
}

pub async fn delete_message(config: &SdkConfig, url: &str, receipt: &str) -> Result<()> {
    aws_sdk_sqs::Client::new(config)
        .delete_message()
        .queue_url(url)
        .receipt_handle(receipt)
        .send()
        .await
        .context("DeleteMessage")?;
    Ok(())
}

fn json_object_to_item(json: &str) -> Result<std::collections::HashMap<String, AttributeValue>> {
    let value: Value = serde_json::from_str(json).context("invalid JSON")?;
    let Value::Object(map) = value else {
        bail!("expected a JSON object");
    };
    map.iter()
        .map(|(k, v)| Ok((k.clone(), json_to_attr(v)?)))
        .collect()
}

fn json_to_attr(value: &Value) -> Result<AttributeValue> {
    let attr = match value {
        Value::String(s) => AttributeValue::S(s.clone()),
        Value::Number(n) => AttributeValue::N(n.to_string()),
        Value::Bool(b) => AttributeValue::Bool(*b),
        Value::Null => AttributeValue::Null(true),
        Value::Array(items) => {
            AttributeValue::L(items.iter().map(json_to_attr).collect::<Result<Vec<_>>>()?)
        }
        Value::Object(map) => AttributeValue::M(
            map.iter()
                .map(|(k, v)| Ok((k.clone(), json_to_attr(v)?)))
                .collect::<Result<_>>()?,
        ),
    };
    Ok(attr)
}

fn attr_to_json(attr: &AttributeValue) -> Value {
    match attr {
        AttributeValue::S(s) => Value::String(s.clone()),
        AttributeValue::N(n) => {
            serde_json::from_str::<Value>(n).unwrap_or_else(|_| Value::String(n.clone()))
        }
        AttributeValue::Bool(b) => Value::Bool(*b),
        AttributeValue::Null(_) => Value::Null,
        AttributeValue::L(items) => Value::Array(items.iter().map(attr_to_json).collect()),
        AttributeValue::M(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), attr_to_json(v)))
                .collect(),
        ),
        AttributeValue::Ss(items) => {
            Value::Array(items.iter().map(|s| Value::String(s.clone())).collect())
        }
        AttributeValue::Ns(items) => {
            Value::Array(items.iter().map(|s| Value::String(s.clone())).collect())
        }
        _ => Value::String("<binary>".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aws::client::EmulatorConnection;

    #[tokio::test]
    #[ignore = "requires a running emulator on 127.0.0.1:9297"]
    async fn live_management_roundtrip() {
        let conn = EmulatorConnection::from_port(9297);
        let config = conn.build_aws_config().await.expect("build aws config");

        // S3: bucket + object lifecycle
        create_bucket(&config, "mgmt-test")
            .await
            .expect("create bucket");
        let src = std::env::temp_dir().join("mgmt-upload.txt");
        std::fs::write(&src, b"hello emulator").unwrap();
        put_object(&config, "mgmt-test", "greeting.txt", &src)
            .await
            .expect("put object");
        let objects = list_objects(&config, "mgmt-test")
            .await
            .expect("list objects");
        assert!(
            objects
                .iter()
                .any(|o| o.key == "greeting.txt" && o.size == 14)
        );
        let dest = std::env::temp_dir().join("mgmt-download.txt");
        get_object(&config, "mgmt-test", "greeting.txt", &dest)
            .await
            .expect("get object");
        assert_eq!(std::fs::read(&dest).unwrap(), b"hello emulator");
        delete_object(&config, "mgmt-test", "greeting.txt")
            .await
            .expect("delete object");
        delete_bucket(&config, "mgmt-test")
            .await
            .expect("delete bucket");

        // DynamoDB: table + item lifecycle
        create_table(&config, "mgmt-test", "id", None)
            .await
            .expect("create table");
        let schema = describe_key_schema(&config, "mgmt-test")
            .await
            .expect("describe key schema");
        assert_eq!(schema, vec![("id".to_string(), "HASH".to_string())]);
        put_item(&config, "mgmt-test", r#"{"id":"1","count":42,"ok":true}"#)
            .await
            .expect("put item");
        let items = scan_items(&config, "mgmt-test", 100).await.expect("scan");
        assert_eq!(items.len(), 1);
        let parsed: Value = serde_json::from_str(&items[0]).unwrap();
        assert_eq!(parsed["id"], "1");
        assert_eq!(parsed["count"], 42);
        assert_eq!(parsed["ok"], true);
        delete_item(&config, "mgmt-test", r#"{"id":"1"}"#)
            .await
            .expect("delete item");
        delete_table(&config, "mgmt-test")
            .await
            .expect("delete table");

        // SQS: queue + message lifecycle
        create_queue(&config, "mgmt-test")
            .await
            .expect("create queue");
        let queues = crate::aws::services::fetch_sqs(&config)
            .await
            .expect("list queues");
        let url = queues
            .iter()
            .find(|r| r.name == "mgmt-test")
            .map(|r| r.detail.clone())
            .expect("queue url");
        send_message(&config, &url, "hello queue")
            .await
            .expect("send message");
        let messages = receive_messages(&config, &url).await.expect("receive");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].body, "hello queue");
        delete_message(&config, &url, &messages[0].receipt)
            .await
            .expect("delete message");
        delete_queue(&config, &url).await.expect("delete queue");
    }

    #[tokio::test]
    #[ignore = "requires a running emulator on 127.0.0.1:9297"]
    async fn live_composite_key_table() {
        let conn = EmulatorConnection::from_port(9297);
        let config = conn.build_aws_config().await.expect("build aws config");

        create_table(&config, "cmp-test", "user_id", Some("created_at"))
            .await
            .expect("create composite table");
        let schema = describe_key_schema(&config, "cmp-test")
            .await
            .expect("describe");
        assert_eq!(
            schema,
            vec![
                ("user_id".to_string(), "HASH".to_string()),
                ("created_at".to_string(), "RANGE".to_string()),
            ]
        );

        put_item(
            &config,
            "cmp-test",
            r#"{"user_id":"u1","created_at":"t1","n":1}"#,
        )
        .await
        .expect("put item 1");
        put_item(
            &config,
            "cmp-test",
            r#"{"user_id":"u1","created_at":"t2","n":2}"#,
        )
        .await
        .expect("put item 2");
        let items = scan_items(&config, "cmp-test", 100).await.expect("scan");
        assert_eq!(items.len(), 2);

        let key = r#"{"user_id":"u1","created_at":"t2"}"#;
        delete_item(&config, "cmp-test", key)
            .await
            .expect("delete item");
        let items = scan_items(&config, "cmp-test", 100).await.expect("rescan");
        assert_eq!(items.len(), 1);

        delete_table(&config, "cmp-test")
            .await
            .expect("delete table");
    }

    #[tokio::test]
    #[ignore = "requires a running emulator on 127.0.0.1:9297"]
    async fn live_duplicate_create_precheck() {
        let conn = EmulatorConnection::from_port(9297);
        let config = conn.build_aws_config().await.expect("build aws config");

        assert!(!queue_exists(&config, "dup-test").await.expect("pre check"));
        create_queue(&config, "dup-test").await.expect("create");
        assert!(queue_exists(&config, "dup-test").await.expect("post check"));

        assert!(
            !bucket_exists(&config, "dup-test-bucket")
                .await
                .expect("pre head")
        );
        create_bucket(&config, "dup-test-bucket")
            .await
            .expect("create bucket");
        assert!(
            bucket_exists(&config, "dup-test-bucket")
                .await
                .expect("post head")
        );

        assert!(
            !table_exists(&config, "dup-test-table")
                .await
                .expect("pre describe")
        );
        create_table(&config, "dup-test-table", "id", None)
            .await
            .expect("create table");
        assert!(
            table_exists(&config, "dup-test-table")
                .await
                .expect("post describe")
        );

        delete_table(&config, "dup-test-table")
            .await
            .expect("del table");
        delete_bucket(&config, "dup-test-bucket")
            .await
            .expect("del bucket");
        delete_queue_by_name(&config, "dup-test")
            .await
            .expect("del queue");
    }

    async fn delete_queue_by_name(config: &SdkConfig, name: &str) -> Result<()> {
        let queues = crate::aws::services::fetch_sqs(config).await?;
        let url = queues
            .iter()
            .find(|r| r.name == name)
            .map(|r| r.detail.clone())
            .context("queue url not found")?;
        delete_queue(config, &url).await
    }
}
