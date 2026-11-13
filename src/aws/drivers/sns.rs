use crate::app_state::Resource;
use anyhow::{Context, Result, bail};
use aws_config::SdkConfig;
use aws_sdk_sns::Client;

pub const LABEL: &str = "SNS";

/// What a subscription listing carries where a not-yet-confirmed subscription's ARN
/// would be. AWS reports the same word, and this emulator's `ListSubscriptions` does too,
/// so a pending subscription has no ARN to act on until its endpoint owner confirms it.
pub const PENDING: &str = "PendingConfirmation";

/// The topic's display name: the last `:`-separated segment of
/// `arn:aws:sns:<region>:<account>:<name>`. The full ARN is always kept in
/// `Resource::detail`, the way `fetch_sqs` keeps the queue URL.
fn topic_name(arn: &str) -> String {
    arn.rsplit(':').next().unwrap_or(arn).to_string()
}

/// A subscription as a listing reports it: how and where it is reached in the name, the
/// subscription ARN in the detail so an action can read it back.
fn subscription_resource(sub: &aws_sdk_sns::types::Subscription) -> Resource {
    let protocol = sub.protocol().unwrap_or_default();
    let endpoint = sub.endpoint().unwrap_or_default();
    let arn = sub.subscription_arn().unwrap_or_default();
    let topic = sub.topic_arn().map(topic_name).unwrap_or_default();
    let name = if arn == PENDING {
        format!("{protocol}:{endpoint} (pending confirmation)")
    } else {
        format!("{protocol}:{endpoint} → {topic}")
    };
    Resource {
        name,
        detail: arn.to_string(),
    }
}

/// Attributes as a sorted pair list: the map the service returns is unordered and a
/// detail view needs a stable order.
fn sorted_attributes(
    attributes: Option<&std::collections::HashMap<String, String>>,
) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = attributes
        .map(|map| map.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    pairs.sort();
    pairs
}

/// Every topic this account holds, one `Resource` per topic: the bare topic name in the
/// name, the topic ARN in the detail. `ListTopics` pages, so the token is followed.
pub async fn fetch(config: &SdkConfig) -> Result<Vec<Resource>> {
    Ok(list_topic_arns(config)
        .await?
        .into_iter()
        .map(|arn| Resource {
            detail: arn.clone(),
            name: topic_name(&arn),
        })
        .collect())
}

/// The same listing as `fetch`, as the ARNs themselves.
pub async fn list_topic_arns(config: &SdkConfig) -> Result<Vec<String>> {
    let client = Client::new(config);
    let mut arns = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let mut req = client.list_topics();
        if let Some(t) = token.clone() {
            req = req.next_token(t);
        }
        let resp = req.send().await.context("ListTopics")?;
        arns.extend(
            resp.topics()
                .iter()
                .filter_map(|t| t.topic_arn().map(str::to_string)),
        );
        token = resp.next_token().map(str::to_string);
        if token.is_none() {
            break;
        }
    }
    Ok(arns)
}

/// Whether the topic is there. Duplicate `CreateTopic` answers with success here — it
/// returns the existing topic's ARN. Live tests only; the create path pre-checks
/// through `fetch`.
#[cfg(test)]
pub async fn topic_exists(config: &SdkConfig, topic_arn: &str) -> Result<bool> {
    match Client::new(config)
        .get_topic_attributes()
        .topic_arn(topic_arn)
        .send()
        .await
    {
        Ok(_) => Ok(true),
        Err(e)
            if e.as_service_error()
                .is_some_and(|s| s.is_not_found_exception()) =>
        {
            Ok(false)
        }
        Err(e) => Err(anyhow::Error::new(e).context("GetTopicAttributes")),
    }
}

/// Create a topic and return its ARN. Re-creating a name this account already owns is a
/// success that names the same topic.
pub async fn create_topic(config: &SdkConfig, name: &str) -> Result<String> {
    let resp = Client::new(config)
        .create_topic()
        .name(name)
        .send()
        .await
        .context("CreateTopic")?;
    resp.topic_arn()
        .map(str::to_string)
        .context("CreateTopic returned no topic ARN")
}

/// Delete a topic. The emulator deletes the topic's subscriptions, and the messages
/// delivered to them, with it — so this call alone leaves nothing behind. Deleting a
/// topic that is not there is a success.
pub async fn delete_topic(config: &SdkConfig, topic_arn: &str) -> Result<()> {
    Client::new(config)
        .delete_topic()
        .topic_arn(topic_arn)
        .send()
        .await
        .context("DeleteTopic")?;
    Ok(())
}

/// A topic's attributes as the service reports them: the computed ones (`TopicArn`,
/// `Owner`, `SubscriptionsConfirmed`, `SubscriptionsPending`, `Policy`, …) beside the
/// ones the owner set.
pub async fn get_topic_attributes(
    config: &SdkConfig,
    topic_arn: &str,
) -> Result<Vec<(String, String)>> {
    let resp = Client::new(config)
        .get_topic_attributes()
        .topic_arn(topic_arn)
        .send()
        .await
        .context("GetTopicAttributes")?;
    Ok(sorted_attributes(resp.attributes()))
}

/// Write one topic attribute (`DisplayName`, `Policy`, `DeliveryPolicy`,
/// `ArchivePolicy`, …); an empty value clears a policy-valued one.
pub async fn set_topic_attribute(
    config: &SdkConfig,
    topic_arn: &str,
    attribute_name: &str,
    attribute_value: &str,
) -> Result<()> {
    Client::new(config)
        .set_topic_attributes()
        .topic_arn(topic_arn)
        .attribute_name(attribute_name)
        .attribute_value(attribute_value)
        .send()
        .await
        .context("SetTopicAttributes")?;
    Ok(())
}

/// Publish one message to a topic, optionally with a subject, and return the message id.
///
/// Publishing to a phone number (`PhoneNumber`) or to a mobile-app endpoint (`TargetArn`)
/// is deliberately absent from this driver: the emulator answers those with a 501 because
/// nothing performs an SMS or app delivery, and an action that reports a send which never
/// happened is worse than one that says it cannot.
pub async fn publish(
    config: &SdkConfig,
    topic_arn: &str,
    subject: Option<&str>,
    message: &str,
) -> Result<String> {
    let mut req = Client::new(config)
        .publish()
        .topic_arn(topic_arn)
        .message(message);
    if let Some(subject) = subject {
        req = req.subject(subject);
    }
    let resp = req.send().await.context("Publish")?;
    resp.message_id()
        .map(str::to_string)
        .context("Publish returned no message id")
}

/// Subscribe an endpoint to a topic and return the subscription ARN.
///
/// `protocol` is one of `sqs`, `lambda`, `email`, `email-json`, `sms`, `http`, `https`,
/// `application`, `firehose`, and `endpoint` has to be in the form that protocol wants —
/// a queue or function ARN, an address, a phone number — or the emulator refuses the call
/// as bad input before it writes anything. Only `http`, `https`, `email` and `email-json`
/// end up pending: they reach an address the emulator cannot know the owner of, so their
/// owner has to confirm them, and until then a listing carries [`PENDING`] in place of
/// their ARN. The AWS-resource protocols (`sqs`, `lambda`, `application`, `firehose`,
/// `sms`) name something the caller already owns, so the emulator confirms them on the
/// spot. Being confirmed is not the same as being delivered to: this emulator delivers
/// to SQS queues and Lambda functions, and records a delivery status for the rest.
pub async fn subscribe(
    config: &SdkConfig,
    topic_arn: &str,
    protocol: &str,
    endpoint: &str,
) -> Result<String> {
    let resp = Client::new(config)
        .subscribe()
        .topic_arn(topic_arn)
        .protocol(protocol)
        .endpoint(endpoint)
        // Ask for the ARN even while the subscription is still pending, so the caller has
        // something to describe and unsubscribe with.
        .return_subscription_arn(true)
        .send()
        .await
        .context("Subscribe")?;
    resp.subscription_arn()
        .map(str::to_string)
        .context("Subscribe returned no subscription ARN")
}

/// Confirm a pending subscription with the token its endpoint was sent, and return the
/// subscription ARN.
///
/// The token is not readable through the SNS API: it is what the notification service
/// delivers TO the endpoint (inside the `SubscribeUrl` of the confirmation body), so the
/// dashboard can only pass along what a subscriber reports back to it.
pub async fn confirm_subscription(
    config: &SdkConfig,
    topic_arn: &str,
    token: &str,
) -> Result<String> {
    let resp = Client::new(config)
        .confirm_subscription()
        .topic_arn(topic_arn)
        .token(token)
        .send()
        .await
        .context("ConfirmSubscription")?;
    resp.subscription_arn()
        .map(str::to_string)
        .context("ConfirmSubscription returned no subscription ARN")
}

/// Remove a subscription by ARN. [`PENDING`] is not an ARN, so it is refused here rather
/// than sent to a service that answers it with a bad-parameter error.
pub async fn unsubscribe(config: &SdkConfig, subscription_arn: &str) -> Result<()> {
    if subscription_arn == PENDING {
        bail!("subscription is not confirmed yet, so it has no ARN to unsubscribe");
    }
    Client::new(config)
        .unsubscribe()
        .subscription_arn(subscription_arn)
        .send()
        .await
        .context("Unsubscribe")?;
    Ok(())
}

/// Every subscription of this account, across all topics.
pub async fn list_subscriptions(config: &SdkConfig) -> Result<Vec<Resource>> {
    let client = Client::new(config);
    let mut resources = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let mut req = client.list_subscriptions();
        if let Some(t) = token.clone() {
            req = req.next_token(t);
        }
        let resp = req.send().await.context("ListSubscriptions")?;
        resources.extend(resp.subscriptions().iter().map(subscription_resource));
        token = resp.next_token().map(str::to_string);
        if token.is_none() {
            break;
        }
    }
    Ok(resources)
}

/// The subscriptions of one topic.
pub async fn list_subscriptions_by_topic(
    config: &SdkConfig,
    topic_arn: &str,
) -> Result<Vec<Resource>> {
    let client = Client::new(config);
    let mut resources = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let mut req = client.list_subscriptions_by_topic().topic_arn(topic_arn);
        if let Some(t) = token.clone() {
            req = req.next_token(t);
        }
        let resp = req.send().await.context("ListSubscriptionsByTopic")?;
        resources.extend(resp.subscriptions().iter().map(subscription_resource));
        token = resp.next_token().map(str::to_string);
        if token.is_none() {
            break;
        }
    }
    Ok(resources)
}

/// A subscription's attributes: the computed ones (`TopicArn`, `Protocol`, `Endpoint`,
/// `PendingConfirmation`, `Owner`, …) beside the ones its owner set.
pub async fn get_subscription_attributes(
    config: &SdkConfig,
    subscription_arn: &str,
) -> Result<Vec<(String, String)>> {
    let resp = Client::new(config)
        .get_subscription_attributes()
        .subscription_arn(subscription_arn)
        .send()
        .await
        .context("GetSubscriptionAttributes")?;
    Ok(sorted_attributes(resp.attributes()))
}

/// Write one subscription attribute (`RawMessageDelivery`, `FilterPolicy`,
/// `DeliveryPolicy`, `ReplayPolicy`, …).
pub async fn set_subscription_attribute(
    config: &SdkConfig,
    subscription_arn: &str,
    attribute_name: &str,
    attribute_value: &str,
) -> Result<()> {
    Client::new(config)
        .set_subscription_attributes()
        .subscription_arn(subscription_arn)
        .attribute_name(attribute_name)
        .attribute_value(attribute_value)
        .send()
        .await
        .context("SetSubscriptionAttributes")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aws::client::EmulatorConnection;

    /// A name no sibling run can collide with, and inside the topic-name rules.
    fn test_topic_name(tag: &str) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        format!("dashboard-sns-{tag}-{}-{nanos}", std::process::id())
    }

    fn absent_topic_arn(name: &str) -> String {
        format!("arn:aws:sns:us-east-1:000000000000:{name}-absent")
    }

    #[tokio::test]
    #[ignore = "requires a running emulator on 127.0.0.1:9297; creates and deletes a topic"]
    async fn live_sns_lifecycle() {
        let conn = EmulatorConnection::from_port(9297);
        let config = conn.build_aws_config().await.expect("aws config");
        let name = test_topic_name("main");
        let topic_arn = create_topic(&config, &name).await.expect("create topic");
        assert!(topic_arn.ends_with(&name), "arn: {topic_arn}");

        // Duplicate CreateTopic is idempotent here, so it answers with the SAME ARN
        // rather than an error — `topic_exists` is what the dashboard asks first.
        let again = create_topic(&config, &name).await.expect("re-create");
        assert_eq!(again, topic_arn);
        assert!(
            topic_exists(&config, &topic_arn)
                .await
                .expect("topic exists")
        );
        assert!(
            !topic_exists(&config, &absent_topic_arn(&name))
                .await
                .expect("absent topic exists")
        );

        let listed = fetch(&config).await.expect("list topics");
        assert!(
            listed
                .iter()
                .any(|r| r.name == name && r.detail == topic_arn),
            "topics: {}",
            resources_text(&listed)
        );
        assert!(
            list_topic_arns(&config)
                .await
                .expect("list topic arns")
                .contains(&topic_arn)
        );

        let message_id = publish(&config, &topic_arn, Some("dashboard subject"), "hello sns")
            .await
            .expect("publish");
        assert!(!message_id.is_empty());

        let attrs = get_topic_attributes(&config, &topic_arn)
            .await
            .expect("get topic attributes");
        assert!(
            attrs
                .iter()
                .any(|(k, v)| k == "TopicArn" && v == &topic_arn),
            "topic attributes: {attrs:?}"
        );

        // The `sqs` protocol addresses a resource the caller owns, so the emulator
        // confirms the subscription at once and it has an ARN to act on. No queue fixture
        // is needed: the endpoint is only held to its ARN form.
        let queue_arn = format!("arn:aws:sqs:us-east-1:000000000000:{name}-target");
        let subscription_arn = subscribe(&config, &topic_arn, "sqs", &queue_arn)
            .await
            .expect("subscribe");
        assert!(
            subscription_arn.starts_with("arn:aws:sns:"),
            "subscription: {subscription_arn}"
        );

        let subs = list_subscriptions_by_topic(&config, &topic_arn)
            .await
            .expect("list subscriptions by topic");
        assert!(
            subs.iter()
                .any(|r| r.detail == subscription_arn && r.name.contains("sqs:")),
            "subscriptions: {}",
            resources_text(&subs)
        );
        assert!(
            list_subscriptions(&config)
                .await
                .expect("list subscriptions")
                .iter()
                .any(|r| r.detail == subscription_arn),
            "account-wide listing is missing {subscription_arn}"
        );

        let sub_attrs = get_subscription_attributes(&config, &subscription_arn)
            .await
            .expect("get subscription attributes");
        assert!(
            sub_attrs
                .iter()
                .any(|(k, v)| k == "PendingConfirmation" && v == "false"),
            "subscription attributes: {sub_attrs:?}"
        );

        // Everything beyond the topic itself is undone here, and the topic delete below
        // runs whatever these steps do — DeleteTopic takes its subscriptions with it, so a
        // mid-test failure still leaves no residue.
        let outcome = async {
            set_subscription_attribute(&config, &subscription_arn, "RawMessageDelivery", "true")
                .await
                .context("set RawMessageDelivery")?;
            let after = get_subscription_attributes(&config, &subscription_arn)
                .await
                .context("re-read subscription attributes")?;
            anyhow::ensure!(
                after
                    .iter()
                    .any(|(k, v)| k == "RawMessageDelivery" && v == "true"),
                "subscription attributes after set: {after:?}"
            );

            set_topic_attribute(&config, &topic_arn, "DisplayName", "dashboard-display")
                .await
                .context("set DisplayName")?;
            let after = get_topic_attributes(&config, &topic_arn)
                .await
                .context("re-read topic attributes")?;
            anyhow::ensure!(
                after
                    .iter()
                    .any(|(k, v)| k == "DisplayName" && v == "dashboard-display"),
                "topic attributes after set: {after:?}"
            );

            unsubscribe(&config, &subscription_arn)
                .await
                .context("unsubscribe")?;
            anyhow::ensure!(
                !list_subscriptions_by_topic(&config, &topic_arn)
                    .await
                    .context("list after unsubscribe")?
                    .iter()
                    .any(|r| r.detail == subscription_arn),
                "subscription {subscription_arn} survived Unsubscribe"
            );

            // Refusals are real and must come back as errors, never as a silent success.
            for (what, refusal) in [
                (
                    "ConfirmSubscription with a token nobody was sent",
                    confirm_subscription(&config, &topic_arn, "not-a-real-token")
                        .await
                        .map(|_| ()),
                ),
                (
                    "Publish with an empty message",
                    publish(&config, &topic_arn, None, "").await.map(|_| ()),
                ),
                (
                    "Publish to a topic that is not there",
                    publish(&config, &absent_topic_arn(&name), None, "hello")
                        .await
                        .map(|_| ()),
                ),
                (
                    "Subscribe with an unknown protocol",
                    subscribe(&config, &topic_arn, "carrier-pigeon", "x@y.com")
                        .await
                        .map(|_| ()),
                ),
                (
                    "Unsubscribe the pending sentinel",
                    unsubscribe(&config, PENDING).await.map(|_| ()),
                ),
                (
                    "GetTopicAttributes of a topic that is not there",
                    get_topic_attributes(&config, &absent_topic_arn(&name))
                        .await
                        .map(|_| ()),
                ),
            ] {
                anyhow::ensure!(refusal.is_err(), "{what} was accepted");
            }
            Ok::<_, anyhow::Error>(())
        }
        .await;

        delete_topic(&config, &topic_arn)
            .await
            .expect("cleanup: delete topic");
        outcome.expect("lifecycle");

        assert!(
            !topic_exists(&config, &topic_arn)
                .await
                .expect("exists after delete"),
            "topic {topic_arn} survived DeleteTopic"
        );
        assert!(
            !fetch(&config)
                .await
                .expect("list after delete")
                .iter()
                .any(|r| r.detail == topic_arn)
        );
    }

    /// What the emulator actually answers for the corners of SNS this driver does not
    /// expose, and for the calls it does. The printout is the record of the wording.
    #[tokio::test]
    #[ignore = "requires a running emulator on 127.0.0.1:9297; creates and deletes a topic"]
    async fn live_sns_capability_matrix() {
        let conn = EmulatorConnection::from_port(9297);
        let config = conn.build_aws_config().await.expect("aws config");
        let name = test_topic_name("matrix");
        let topic_arn = create_topic(&config, &name).await.expect("create topic");
        let client = Client::new(&config);

        println!(
            "{}",
            result(
                "ListTopics",
                fetch(&config).await.map(|r| resources_text(&r))
            )
        );
        println!(
            "{}",
            result(
                "Publish(PhoneNumber)",
                client
                    .publish()
                    .phone_number("+15551234567")
                    .message("sms is not emulated")
                    .send()
                    .await
                    .map(|_| ()),
            )
        );
        println!(
            "{}",
            result(
                "Publish(TopicArn + PhoneNumber)",
                client
                    .publish()
                    .topic_arn(&topic_arn)
                    .phone_number("+15551234567")
                    .message("two destinations")
                    .send()
                    .await
                    .map(|_| ()),
            )
        );
        println!(
            "{}",
            result(
                "Publish(no destination)",
                client.publish().message("nowhere").send().await.map(|_| ()),
            )
        );
        println!(
            "{}",
            result(
                "Publish(empty message)",
                publish(&config, &topic_arn, None, "").await.map(|_| ()),
            )
        );
        println!(
            "{}",
            result(
                "Publish(subject 101 chars)",
                publish(&config, &topic_arn, Some(&"s".repeat(101)), "hello")
                    .await
                    .map(|_| ()),
            )
        );
        println!(
            "{}",
            result(
                "Publish(other region topic)",
                publish(
                    &config,
                    "arn:aws:sns:eu-west-1:000000000000:x",
                    None,
                    "hello"
                )
                .await
                .map(|_| ()),
            )
        );
        println!(
            "{}",
            result(
                "PublishBatch(2 entries)",
                client
                    .publish_batch()
                    .topic_arn(&topic_arn)
                    .publish_batch_request_entries(batch_entry("m1", "first"))
                    .publish_batch_request_entries(batch_entry("m2", "second"))
                    .send()
                    .await
                    .map(|r| format!("failed entries: {:?}", r.failed())),
            )
        );
        println!(
            "{}",
            result(
                "CreateTopic(invalid name)",
                create_topic(&config, "not a valid topic name")
                    .await
                    .map(|_| ()),
            )
        );
        println!(
            "{}",
            result(
                "SetTopicAttributes(unknown attribute)",
                set_topic_attribute(&config, &topic_arn, "NoSuchAttribute", "1")
                    .await
                    .map(|_| ()),
            )
        );
        println!(
            "{}",
            result(
                "Subscribe(unknown protocol)",
                subscribe(&config, &topic_arn, "carrier-pigeon", "x@example.com")
                    .await
                    .map(|_| ()),
            )
        );
        println!(
            "{}",
            result(
                "Subscribe(email, malformed address)",
                subscribe(&config, &topic_arn, "email", "not-an-address")
                    .await
                    .map(|_| ()),
            )
        );
        println!(
            "{}",
            result(
                "Subscribe(sqs, not an ARN)",
                subscribe(&config, &topic_arn, "sqs", "my-queue")
                    .await
                    .map(|_| ()),
            )
        );
        println!(
            "{}",
            result(
                "Subscribe(topic that is not there)",
                subscribe(
                    &config,
                    &absent_topic_arn(&name),
                    "sqs",
                    "arn:aws:sqs:us-east-1:000000000000:q",
                )
                .await
                .map(|_| ()),
            )
        );

        let pending = subscribe(&config, &topic_arn, "email", "someone@example.com").await;
        println!("Subscribe(email) -> {pending:?}");
        let listed = list_subscriptions_by_topic(&config, &topic_arn)
            .await
            .expect("list subscriptions");
        let pending_entry = listed
            .iter()
            .find(|r| r.name.contains("email"))
            .map(|r| format!("{} | {}", r.name, r.detail));
        println!("ListSubscriptionsByTopic, pending entry -> {pending_entry:?}");
        println!(
            "{}",
            result(
                "GetSubscriptionAttributes(the pending sentinel)",
                get_subscription_attributes(&config, PENDING)
                    .await
                    .map(|_| ()),
            )
        );
        println!(
            "{}",
            result(
                "ConfirmSubscription(unknown token)",
                confirm_subscription(&config, &topic_arn, "an-unknown-token")
                    .await
                    .map(|_| ()),
            )
        );
        println!(
            "{}",
            result(
                "ListTagsForResource",
                client
                    .list_tags_for_resource()
                    .resource_arn(&topic_arn)
                    .send()
                    .await
                    .map(|r| format!("tags: {:?}", r.tags())),
            )
        );
        println!(
            "{}",
            result(
                "DeleteTopic(unknown topic)",
                delete_topic(&config, &absent_topic_arn(&name))
                    .await
                    .map(|_| ()),
            )
        );

        delete_topic(&config, &topic_arn)
            .await
            .expect("cleanup: delete topic");
    }

    /// `Resource` carries no `Debug` (it belongs to another module), so a listing is
    /// printed as the pairs the dashboard actually shows.
    fn resources_text(items: &[Resource]) -> String {
        items
            .iter()
            .map(|r| format!("{} | {}", r.name, r.detail))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn batch_entry(id: &str, message: &str) -> aws_sdk_sns::types::PublishBatchRequestEntry {
        aws_sdk_sns::types::PublishBatchRequestEntry::builder()
            .id(id)
            .message(message)
            .build()
            .expect("batch entry")
    }

    /// `OK` beside whatever the call reports, or the failure's Display and Debug forms:
    /// the emulator's own message reaches the caller through the SDK error, so the wording
    /// is printed as observed rather than paraphrased.
    fn result<T: std::fmt::Debug, E: std::fmt::Display + std::fmt::Debug>(
        name: &str,
        outcome: std::result::Result<T, E>,
    ) -> String {
        match outcome {
            Ok(value) => format!("{name}: OK -> {value:?}"),
            Err(e) => format!("{name}: FAIL display=[{e}] debug=[{e:?}]"),
        }
    }
}
