#!/usr/bin/env python3
"""Regenerate SERVICES.md from assets/services.json.

    python3 tools/gen-services-doc.py > SERVICES.md

The managed set is read from src/views/catalog.rs so the document cannot drift from the code.
"""
import json
import re
import pathlib

root = pathlib.Path(__file__).resolve().parent.parent
services = json.loads((root / "assets" / "services.json").read_text())
catalog = (root / "src" / "views" / "catalog.rs").read_text()

block = re.search(r"pub const MANAGED:.*?\n\];", catalog, re.S).group(0)
managed = {}
for key in re.findall(r'\(\s*"([a-z0-9]+)"', block, re.S):
    managed[key] = True

create_block = re.search(
    r"pub fn create_action.*?\n\}\n", catalog, re.S
).group(0)
create_keys = set(re.findall(r'"([a-z0-9]+)" => Some\(', create_block))

DRIVER = {
    "s3": "src/aws/services.rs",
    "dynamodb": "src/aws/services.rs",
    "sqs": "src/aws/services.rs",
    "ec2": "src/aws/drivers/ec2.rs",
    "iam": "src/aws/drivers/iam.rs",
    "sns": "src/aws/drivers/sns.rs",
    "rds": "src/aws/drivers/rds.rs",
    "logs": "src/aws/drivers/cloudwatch_logs.rs",
    "kinesis": "src/aws/drivers/kinesis.rs",
    "secretsmanager": "src/aws/drivers/secrets_manager.rs",
    "monitoring": "src/aws/drivers/cloudwatch.rs",
    "sts": "src/aws/drivers/sts.rs",
    "ssm": "src/aws/drivers/ssm.rs",
}
LABEL = {
    "s3": "S3 Buckets",
    "dynamodb": "DynamoDB",
    "sqs": "SQS",
    "ec2": "EC2",
    "iam": "IAM",
    "sns": "SNS",
    "rds": "RDS",
    "logs": "CloudWatch Logs",
    "kinesis": "Kinesis",
    "secretsmanager": "Secrets Manager",
    "monitoring": "CloudWatch",
    "sts": "STS",
    "ssm": "SSM Parameters",
}
NOUN = {
    "s3": "bucket",
    "dynamodb": "table",
    "sqs": "queue",
    "ec2": "instance",
    "iam": "user",
    "sns": "topic",
    "rds": "DB instance",
    "logs": "log group",
    "kinesis": "stream",
    "secretsmanager": "secret",
    "monitoring": "alarm",
    "sts": "session",
    "ssm": "parameter",
}


def status(e):
    if e["signingName"] in managed:
        return "✅"
    return "🚧" if e["registered"] else "❌"


def word(e):
    if e["signingName"] in managed:
        return "managed"
    return "served, no page" if e["registered"] else "not in build"


managed_rows = [e for e in services if e["signingName"] in managed]
managed_ops = sum(e["ops"] for e in managed_rows)
served = [e for e in services if e["registered"]]
uncovered = [e for e in served if e["signingName"] not in managed]
absent = [e for e in services if not e["registered"]]

out = []
w = out.append

w("# Service coverage")
w("")
w("Every service in the baked catalog — [assets/services.json](assets/services.json), "
  f"{len(services)} rows — and whether a page in this dashboard can manage it. This is the "
  "worklist for ROADMAP M1; regenerate it with `python3 tools/gen-services-doc.py > SERVICES.md` "
  "so it never disagrees with [src/views/catalog.rs:11](src/views/catalog.rs#L11).")
w("")
w("Measured 2026-11-14 against the emulator's 338-service build.")
w("")
w("## Where things stand")
w("")
w("| | count | operations | meaning |")
w("|---|---|---|---|")
w(f"| ✅ managed | {len(managed_rows)} | {managed_ops:,} | a driver lists its resources and a page acts on them |")
w(f"| 🚧 served, no page | {len(uncovered)} | {sum(e['ops'] for e in uncovered):,} | the emulator answers it; the focus panel says so and offers nothing |")
w(f"| ❌ not in build | {len(absent)} | — | `registered: false`; the emulator refuses it |")
w(f"| **catalog total** | **{len(services)}** | **{sum(e['ops'] for e in served):,}** | |")
w("")
w("Status marks:")
w("")
w("- **✅ managed** — in `MANAGED`, with a driver under `src/aws/drivers/` (or "
  "[src/aws/services.rs](src/aws/services.rs) for S3, DynamoDB and SQS, which predate that "
  "layout) and a page reachable from the sidebar.")
w("- **🚧 served, no page** — `registered: true`, so `aws --endpoint-url` works against it, but "
  "`render_operations` returns nothing because `is_managed` is false "
  "([src/views/detail.rs:1251](src/views/detail.rs#L1251)).")
w("- **❌ not in build** — `registered: false`. Not a dashboard gap; the emulator image does not "
  "contain that service.")
w("")
w(f"## The {len(managed_rows)} that are managed")
w("")
w("| service | page | driver | ops in service | create action | resource noun |")
w("|---|---|---|---|---|---|")
for e in sorted(managed_rows, key=lambda e: -e["ops"]):
    k = e["signingName"]
    created = "yes" if k in create_keys else "read-only"
    w(f"| `{k}` | {LABEL[k]} | [{DRIVER[k]}]({DRIVER[k]}) "
      f"| {e['ops']} | {created} | {NOUN[k]} |")
w("")
w("Every one of these is served by the emulator (`registered: true`), and together they hold "
  f"{managed_ops:,} of the {sum(e['ops'] for e in served):,} operations the build answers — "
  f"{managed_ops / sum(e['ops'] for e in served) * 100:.1f}%.")
w("")
w("## By category")
w("")
w("Services are grouped by the taxonomy this project chose for the sidebar "
  "(`category` in `assets/services.json`, our label, not AWS's). Within a group: managed "
  "first, then served-but-uncovered by operation count, then the services this emulator build "
  "does not contain. Operation count is a size measure, not a verdict on what to build next — "
  "see *Where to start*.")
w("")

categories = {}
for e in services:
    categories.setdefault(e["category"], []).append(e)


def sort_key(e):
    return (
        0 if e["signingName"] in managed else (1 if e["registered"] else 2),
        -e["ops"],
        e["signingName"],
    )


for cat in sorted(categories, key=lambda c: -len([x for x in services if x["category"] == c])):
    rows = sorted(categories[cat], key=sort_key)
    n_managed = sum(1 for e in rows if e["signingName"] in managed)
    w(f"### {cat} — {len(rows)} services, {n_managed} managed")
    w("")
    w("| status | service | signing name | protocol | ops |")
    w("|---|---|---|---|---|")
    for e in rows:
        w(f"| {status(e)} | {e['title']} | `{e['signingName']}` | {e['protocol']} | {e['ops']} |")
    w("")

w("## Where to start")
w("")
w("Two different orders, and choosing between them is the M1 decision.")
w("")
w("**Raw operation count.** 🚧 services with the most operations. Read this honestly: the top "
  "of the list is Glue, QuickSight and IoT — services nobody provisions on a laptop. Volume is "
  "not the same as usefulness.")
w("")
w("| rank | status | service | signing name | ops | category |")
w("|---|---|---|---|---|---|")
for i, e in enumerate(sorted(uncovered, key=lambda e: -e["ops"])[:25], start=1):
    w(f"| {i} | 🚧 | {e['title']} | `{e['signingName']}` | {e['ops']} | {e['category']} |")
w("")
w(f"The remaining {len(uncovered) - 25} 🚧 services are in the category tables above.")
w("")
w("**What a local emulator user reaches for.** ROADMAP M1 option A's curated set, where "
  "operation count is a tie-break rather than the sort key. The membership is the roadmap's "
  "judgement; the counts below come from the catalog, so they cannot go stale.")
w("")
w("| service | signing name | ops | category |")
w("|---|---|---|---|")
CURATED = [
    "config", "networkmanager", "workspaces", "cloudformation", "elasticache",
    "route53", "athena", "autoscaling", "cloudtrail", "codebuild", "ecr", "events", "kms",
    "elasticloadbalancing",
]
by_name = {e["signingName"]: e for e in services}
for k in CURATED:
    e = by_name[k]
    assert e["registered"] and e["signingName"] not in managed, k
    w(f"| {e['title']} | `{k}` | {e['ops']} | {e['category']} |")
w("")
w("Each is one `aws-sdk-*` crate, one driver file, one `MANAGED` entry, and rows in "
  "[src/views/operations.rs](src/views/operations.rs); the pattern is proven eleven times over.")
w("")
w(f"Beyond the 25 above, {len(uncovered) - 25} more 🚧 services are waiting in the category "
  "tables, and no order reaches them for free. A generic runner needs a crate per service "
  "anyway, because the AWS SDK is per-service — see ROADMAP Q1 for why that is an open "
  "decision, not a solved one.")

print("\n".join(out))
