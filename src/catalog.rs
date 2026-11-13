use serde::Deserialize;
use std::sync::OnceLock;

/// One emulator service, as baked into `assets/services.json`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Service {
    pub signing_name: String,
    pub title: String,
    pub sdk_id: String,
    #[serde(rename = "crate")]
    pub crate_name: String,
    pub protocol: Option<String>,
    pub version: Option<String>,
    pub ops: usize,
    pub registered: bool,
    pub category: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Sort {
    #[default]
    Name,
    Ops,
}

impl Sort {
    pub const ALL: [Sort; 2] = [Sort::Name, Sort::Ops];

    pub fn label(self) -> &'static str {
        match self {
            Sort::Name => "Name",
            Sort::Ops => "Operations",
        }
    }

    fn apply(self, a: &Service, b: &Service) -> std::cmp::Ordering {
        match self {
            Sort::Name => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
            Sort::Ops => b.ops.cmp(&a.ops).then_with(|| a.title.cmp(&b.title)),
        }
    }
}

pub fn all() -> &'static [Service] {
    static CATALOG: OnceLock<Vec<Service>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/services.json"))
            .expect("assets/services.json must be a valid service catalog")
    })
}

pub fn find(signing_name: &str) -> Option<&'static Service> {
    all().iter().find(|s| s.signing_name == signing_name)
}

pub struct Group {
    pub name: String,
    pub items: Vec<&'static Service>,
    pub total: usize,
}

fn matches(s: &Service, q: &str) -> bool {
    q.is_empty()
        || s.title.to_lowercase().contains(q)
        || s.signing_name.to_lowercase().contains(q)
        || s.sdk_id.to_lowercase().contains(q)
}

/// Every category that has at least one hit, with its hits sorted.
pub fn browse(query: &str, sort: Sort) -> Vec<Group> {
    let q = query.trim().to_lowercase();
    let mut names: Vec<String> = all().iter().map(|s| s.category.clone()).collect();
    names.sort();
    names.dedup();
    names
        .iter()
        .filter_map(|name| {
            let mut items: Vec<&Service> = all()
                .iter()
                .filter(|s| &s.category == name && matches(s, &q))
                .collect();
            if items.is_empty() {
                return None;
            }
            items.sort_by(|a, b| sort.apply(a, b));
            Some(Group {
                total: all().iter().filter(|s| &s.category == name).count(),
                name: name.clone(),
                items,
            })
        })
        .collect()
}

pub fn matched(query: &str) -> usize {
    let q = query.trim().to_lowercase();
    all().iter().filter(|s| matches(s, &q)).count()
}

/// `CloudWatch Logs` -> `cloudwatch_logs`: the driver module and file name.
pub fn driver_module(sdk_id: &str) -> String {
    let mut out = String::new();
    for c in sdk_id.chars() {
        if c.is_ascii_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

/// `CloudWatch Logs` -> `aws-sdk-cloudwatchlogs`: the official SDK crate.
/// The SDK drops the brand word from the product name, so `Amazon DynamoDB` is `aws-sdk-dynamodb`.
pub fn sdk_crate(sdk_id: &str) -> String {
    let unbranded = ["Amazon ", "Aws ", "AWS "]
        .iter()
        .find_map(|brand| sdk_id.strip_prefix(brand))
        .unwrap_or(sdk_id);
    format!("aws-sdk-{}", driver_module(unbranded).replace('_', ""))
}

pub fn counts() -> (usize, usize, usize) {
    let all = all();
    (
        all.len(),
        all.iter().filter(|s| s.registered).count(),
        all.iter().map(|s| s.ops).sum(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_baked_and_consistent() {
        let (total, registered, ops) = counts();
        assert_eq!(total, all().len());
        assert!(registered <= total);
        assert!(
            ops > 10_000,
            "expected a five-figure operation total, got {ops}"
        );
        assert!(
            all()
                .iter()
                .all(|s| !s.signing_name.is_empty() && !s.category.is_empty()),
            "every row needs a signing name and a category"
        );
    }

    #[test]
    fn browse_covers_every_service_once() {
        let groups = browse("", Sort::Name);
        let in_groups: usize = groups.iter().map(|g| g.items.len()).sum();
        assert_eq!(in_groups, all().len());
        assert!(
            groups.len() > 1,
            "catalog must be grouped, got one flat list"
        );
        for g in &groups {
            assert!(!g.items.is_empty());
            assert_eq!(g.total, g.items.len(), "empty query matches everything");
            assert!(g.items.iter().all(|s| s.category == g.name));
        }
    }

    #[test]
    fn query_filters_within_groups() {
        let groups = browse("cloudwatch", Sort::Name);
        let hits: Vec<&Service> = groups.iter().flat_map(|g| g.items.clone()).collect();
        assert!(hits.iter().any(|s| s.signing_name == "logs"));
        assert!(hits.iter().any(|s| s.signing_name == "monitoring"));
        assert!(
            hits.iter()
                .all(|s| s.title.to_lowercase().contains("cloudwatch")),
            "a query must not leave unmatched services in a group"
        );
        assert_eq!(
            matched("s3"),
            all().iter().filter(|s| matches(s, "s3")).count()
        );
        assert!(
            matched("s3") > 1 && matched("s3") < all().len(),
            "a short query should narrow the catalog without emptying it"
        );
        assert_eq!(matched("  "), all().len());
        assert_eq!(matched("zzzz-not-a-service"), 0);
        assert!(browse("zzzz-not-a-service", Sort::Name).is_empty());
    }

    #[test]
    fn ops_sort_is_descending_within_each_group() {
        let groups = browse("", Sort::Ops);
        for g in &groups {
            assert!(
                g.items.windows(2).all(|w| w[0].ops >= w[1].ops),
                "group {} is not sorted by operation count",
                g.name
            );
        }
        let busiest = all()
            .iter()
            .max_by_key(|s| s.ops)
            .expect("catalog is not empty");
        assert_eq!(busiest.signing_name, "ec2");
    }

    #[test]
    fn unregistered_services_are_flagged() {
        let lambda = find("lambda").expect("lambda is modelled");
        assert!(
            !lambda.registered,
            "the running build does not serve lambda"
        );
        assert!(find("s3").expect("s3").registered);
        assert!(find("not-a-service").is_none());
    }

    #[test]
    fn driver_and_crate_names_are_derived() {
        assert_eq!(driver_module("EC2"), "ec2");
        assert_eq!(driver_module("CloudWatch Logs"), "cloudwatch_logs");
        assert_eq!(driver_module("Secrets Manager"), "secrets_manager");
        assert_eq!(sdk_crate("CloudWatch Logs"), "aws-sdk-cloudwatchlogs");
        assert_eq!(sdk_crate("Secrets Manager"), "aws-sdk-secretsmanager");
        assert_eq!(sdk_crate("Amazon DynamoDB"), "aws-sdk-dynamodb");
    }
}
