# Console v2 — Specification

Target: `/Users/snapp/projects/aws-emulator-dashboard`, the native Rust + GPUI desktop
dashboard for the local AWS emulator at `https://127.0.0.1:9297`.

Scope rule that drives every decision below (user, verbatim): **"every thing this provide
must be managable using the dashboard."**

## Non-negotiable constraints

| # | Constraint | Consequence |
|---|---|---|
| C1 | **"dont implement the sdk yourself, use aws rust sdk"** — official `aws-sdk-*` crates only, no hand-rolled SigV4 or protocol layer in the product | Every managed service needs its own SDK crate compiled into the app. This *bounds* the tier (see §Why not 338 pages). |
| C2 | Emulator container: **"I will not touch it"** | The dashboard never rebuilds/restarts the running container to gain a capability. It reports what the running build actually serves. |
| C3 | Panels with no emulator data source: **"Keep layout, mark unavailable"** | Nothing is faked. A panel renders its real shape and states *why* it has no data. |

## Ground truth (measured, not assumed)

All of this was measured against the container running now, `aws-emulator-9297`.

| Fact | Value | How it was obtained |
|---|---|---|
| Service crates in the emulator repo | 436 | `grep -c register_service` in `aws-emulator/core/router/src/lib.rs` |
| Unique SigV4 signing names | 364 | extracted from the per-crate Smithy models |
| Signing names the running container answers | **338** | one signed request per name; 26 answered `404 UnknownService` |
| Operations modelled across all services | 14,502 | sum of operation shapes per service |
| Protocols | restJson1 260, awsJson1_1 102, awsJson1_0 51, awsQuery 15, restXml 4, ec2Query 1, none 3 | same extraction |
| **`lambda` is not served** | `404 {"__type":"UnknownService","message":"no emulated service is registered for 'lambda'"}` | live probe |
| Unregistered names include | `agent-registry`, `apigateway`, `appconfig`, `aws-marketplace`, `bedrock`, `chime`, `connect`, `es`, `execute-api`, `forecast`, `lambda`, `lex`, `mgh`, `payment-cryptography`, `personalize`, `resiliencehub`, `sagemaker`, `servicecatalog`, `sms-voice`, `wisdom` (+6) | live probe |
| Duplicate creates are **not** rejected | emulator returns success for a second CreateBucket/CreateTable/CreateQueue | live SDK test, see [src/aws/manage.rs:1](../src/aws/manage.rs) header comment |
| Validation is real | EC2 `InvalidAMIID.Malformed`; RDS `Invalid value 5 for MaxRecords. Must be between 20 and 100` | live SDK calls |
| EC2 state machine is clock-driven | starting an instance before it reaches `stopped` returns `IncorrectInstanceState` | live SDK test |
| Unhandled operations answer 501 | `{"__type":"NotImplemented","message":"operation … is not implemented in this emulator"}` | live probe |
| "Implemented:" markers in op files are **not** a coverage signal | only 634/19,826 files (3.2%) carry the marker, yet dynamodb/sqs/iam/s3 all demonstrably work | repo scan vs live behaviour |
| Adding 10 SDK crates costs +100 s `cargo check`; 13 crates → 904 lock packages, 15 GB `target/debug` | measured | cost model for §Why not 338 pages |

## Why not 338 first-class pages

C1 forces one `aws-sdk-<service>` crate per managed service. Extrapolating the measured
marginal cost (≈10 s/check/crate, and each crate is 100–800 operations of generated code),
338 crates is a multi-hour cold build and a target directory far beyond this machine. That
is not a style preference — it is the measured price.

So the dashboard is three tiers, and the tiers are visible in the UI rather than hidden:

```
Tier A  first-class pages   11 SDK-backed services   real fetch + real management + live tests
Tier B  catalog             all 364 names            browsable, searchable, honest status
Tier C  raw traffic         proxy 127.0.0.1:9298     every signed call, already implemented
```

**Tier A** (`s3`, `dynamodb`, `sqs`, `ec2`, `iam`, `sns`, `logs`, `monitoring`, `rds`,
`kinesis`, `secretsmanager` — every one verified `registered: true`):
sidebar entry, resource list, per-resource detail, create/delete/act dialogs, live test.

**Tier B**: `GET`-free static browse of [assets/services.json](../assets/services.json) —
92 KB baked at build time via `include_str!`, one entry per signing name with
`title / sdkId / crate / protocol / version / ops / registered / category`. Selecting a
Tier-B service opens a page that says what it *would* take: the crate to add and the driver
file to write. No SDK calls, so no fake data.

**Tier C** is the escape hatch that keeps the scope rule honest: with the proxy enabled
([src/emulator/proxy.rs](../src/emulator/proxy.rs)), a user can send any request to any
Tier-B service and see it in the traffic log, even before a page exists.

## Categories

`assets/services.json` carries a `category` per service — **our taxonomy, not AWS's**
(the Smithy models contain no category field). Derived by exact-name overrides plus
start-of-name prefix rules, then reviewed; 17 groups, largest = Management & Governance
(63), smallest = End User Computing (6), and 24 names sit in `Other` because they are not
confidently classifiable (`aps`, `geo-*`, `partnercentral-*`, `scn`, `uxc`…). Regenerating
is `python3 mkcatalog.py` — the rules are data, not prose.

## Pages

### 1. Overview (new)

Real aggregations only; each card names its source.

| Panel | Source | Status |
|---|---|---|
| Stat cards: services, registered, resources, requests | `assets/services.json` + Tier A counts + proxy log | 🧭 |
| Recent Activity | `AppState.traffic` (already rendered by [src/views/traffic_log.rs](../src/views/traffic_log.rs)) | 🧭 reuse |
| Alerts | per-service `ServiceResources.error` (501/UnknownService surfaced verbatim) + emulator process state | 🧭 |
| Pinned Services | first 6 Tier A services by resource count | 🧭 |

### 2. Instances (new, EC2-backed)

Backed by [src/aws/drivers/ec2.rs](../src/aws/drivers/ec2.rs) ✅.
State tabs All/Running/Stopped/Terminated filter `Resource.detail` (`"{state} · {type} · {az}"`);
filter box matches id or type; row "…" menu = Start / Stop / Terminate, each calling the
driver and then `wait_state` because of the clock-driven transitions; "Launch Instance"
calls `create_instance`.
The CPU bar has **no emulator data source** → C3: bar rendered, labelled `cpu: not emulated`.

### 3. Services catalog (new)

Backed by `assets/services.json` ✅. "364 services across 17 groups", search over
`title/signingName/sdkId`, sort by name or operation count, grid/list toggle, per-group
counts, `Tier A` badge on the 11 managed services, `not in running build` badge on the 26.
Clicking a Tier A row navigates to that page; a Tier B row opens the "what it would take" panel.

### 4. Existing pages

Control ([src/views/control_panel.rs](../src/views/control_panel.rs)), Traffic
([src/views/traffic_log.rs](../src/views/traffic_log.rs)) and the per-service list/detail
([src/views/detail.rs](../src/views/detail.rs)) stay; the sidebar gains the three pages
above and the Tier A entries replace the hardcoded 3-service list.

## Code changes

| File | Change | Owner |
|---|---|---|
| `assets/services.json` | baked catalog, 364 rows | done ✅ |
| [src/aws/drivers/ec2.rs](../src/aws/drivers/ec2.rs) | pilot: `fetch`, start/stop/terminate, `instance_state`, `wait_state`, `create_instance`, live test | done ✅ |
| `src/aws/drivers/{iam,sns,cloudwatch_logs,cloudwatch,rds}.rs` | one service each, same shape | 🚧 agents |
| `src/aws/drivers/{kinesis,secrets_manager}.rs` | same shape, launched after the first batch lands | 🧭 |
| [src/aws/drivers/mod.rs](../src/aws/drivers/mod.rs) | module declarations | orchestrator only |
| `Cargo.toml` | 11 `aws-sdk-*` crates; `aws-sdk-lambda` removed because C2 says the running build doesn't serve it | orchestrator only |
| [src/app_state.rs](../src/app_state.rs) | `SERVICES` const → a service registry with tier + nav key; `Page` for the three console pages | orchestrator only |
| [src/views/root.rs:15](../src/views/root.rs#L15) | `pub const SERVICES: [(&str, &str); 3]` → registry-driven sidebar with group headers | orchestrator only |
| `src/views/{overview,instances,catalog}.rs` | the three pages | orchestrator only |

Agents are confined to exactly one driver file each and are told not to run `cargo fmt` or
edit shared files, so parallel work cannot collide.

## Testing

Every driver carries `#[tokio::test] #[ignore] live_<service>_lifecycle` against the real
emulator, using the guaranteed-cleanup shape from the pilot so a mid-test failure cannot
leave residue that poisons the next run:

```rust
let outcome = async { /* mutations */ Ok::<_, anyhow::Error>(()) }.await;
delete(&config, &id).await.ok();
outcome.expect("lifecycle");
```

Acceptance for this phase: `cargo test` green, `cargo test -- --ignored` green, the app
launches, all three pages render with live data, and no panel shows a number it cannot name
a source for.

## Honest gaps

1. **Registration is a snapshot.** `registered` was probed from one container on 2026-11-12;
   a rebuilt container can change it. There is no `/__admin/services` endpoint
   (`/__admin/health` is the only one that answers), and a live rescan of 364 names would
   need signing outside the SDK, which C1 forbids. So the bake is dated, and the badge says
   "not in the running build as probed", not "never".
2. **Coverage inside a Tier A service is per-operation, not per-service.** A driver's
   `fetch` may work while 60 of its 80 operations answer 501. Each agent reports an
   operation → error-string table; those tables become the source of the per-service
   capability panel, and anything unimplemented will show the emulator's own words.
3. **11 of 338 services are manageable today.** The scope rule is only satisfiable per-tier:
   Tier A depth, Tier B breadth, Tier C escape hatch.
4. **`ec2` has 807 operations.** The Instances page uses four of them.
5. Overview "Alerts" has no severity model yet — it is the error strings we already capture.
