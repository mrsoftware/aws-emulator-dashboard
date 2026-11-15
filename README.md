# AWS Emulator Dashboard

[![CI](https://github.com/mrsoftware/aws-emulator-dashboard/actions/workflows/ci.yml/badge.svg)](https://github.com/mrsoftware/aws-emulator-dashboard/actions/workflows/ci.yml)

A native Rust desktop console for a local AWS emulator: one window that inventories, manages,
and watches the emulator's in-memory AWS instead of the real one. Built on the `gpui-kit`
crate (GPUI, the Zed UI toolkit) — no Electron, no browser, no web server.

The UI follows the AWS console layout (sidebar of services, resource tables, an operations
card per resource) so the muscle memory transfers. What it talks to is always the emulator.

> **Status: active development.** The API, configuration format, and behavior change between
> commits; macOS is the only tested platform. Do not depend on it in anything stable yet.

## What it does

| Page | What you get |
|---|---|
| **Dashboard** | Service and resource counts, what is served vs baked into the catalog, recent requests |
| **Instances** | EC2 table with state, type, IPv4, launch time; launch, start, stop, terminate |
| **S3 / DynamoDB / SQS / IAM / SNS / RDS / CloudWatch / CloudWatch Logs / Kinesis / Secrets Manager** | Resource list plus an operations card per resource: read-only calls fill the output panel, write calls run against the emulator |
| **All services** | The 364-service catalog, grouped into 17 categories, searchable — managed services show their resources, the rest show what the emulator answers |
| **Emulator control** | Start / stop the emulator (Docker or a local binary), port, auth, telemetry, idle timeout, proxy |
| **Traffic** | Every request the dashboard sent through its own reverse proxy, with status and latency |

`Cmd-R` / `Ctrl-R` refreshes the current view.

Secrets (`Secrets Manager` values, the IAM access key secret) are shown once in the UI and are
never written to a log, a file, or a test output.

## Prerequisites

1. **Rust** — edition 2024, so 1.85 or newer. Developed on 1.99.
2. **A running emulator** on `https://127.0.0.1:9297` (self-signed TLS). Start it from this
   dashboard's *Emulator control* page, or from the emulator project with
   `cargo run -p aws-emulator-router`. It binds `:9297` by default; override with
   `EMULATOR_DATA_PORT`.
3. **Docker** — only if you let the dashboard start and stop the emulator for you. A local
   binary works instead; point `emulator_binary` at it.
4. **The emulator's credential bundle**, which the emulator writes when it boots:
   `~/.aws/emulator/<port>/emulator_credentials` and `~/.aws/emulator/<port>/ca.pem`. The
   dashboard reads both to sign requests and to trust the certificate; it never accepts an
   unsigned or hand-built request.

Without an emulator the app still opens — every page reports the connection failure instead of
showing invented data.

## Build and run

```bash
cargo build
cargo run
```

Windows open at 1200×800. There is no headless mode for the app itself, but the render tests
drive GPUI's headless platform directly and write PNGs to `target/console-render/`.

## Configuration

`Settings::load` reads, and the control page writes, one TOML file:

| Platform | Path |
|---|---|
| macOS | `~/Library/Application Support/aws-emulator-dashboard/config.toml` |
| Linux | `~/.config/aws-emulator-dashboard/config.toml` |

```toml
emulator_binary = "aws-emulator"  # binary the control page runs, if not Docker
default_port = 9297
poll_interval_ms = 5000           # resource refresh cadence
auth_enabled = true               # sign requests with the emulator credentials
telemetry_enabled = false
idle_timeout_secs = 0             # 0 = never
proxy_enabled = false             # route SDK calls through the traffic log
proxy_port = 9298
```

Every key has a default ([src/settings.rs:18](src/settings.rs#L18)); a missing or unparsable
file falls back to those rather than failing to start. Region is fixed at `us-east-1` because
the emulator serves one — see [src/aws/client.rs:8](src/aws/client.rs#L8).

## Tests

```bash
cargo test                              # 25 tests: logic, SDK plumbing, headless render
cargo test -- --ignored                 # 25 more: need a live emulator on :9297
cargo test -- --ignored --list          # see what each one touches before running it
```

The headless render tests draw through CoreText and Metal, so they compile only on macOS: Linux
CI runs 14 tests and ignores 23, macOS runs 25 and ignores 25.

The ignored set is honest about what it changes: some walk a full lifecycle (create, read,
delete) and clean up after themselves; `live_seed_demo_resources` creates demo resources and
leaves them. Run them against a disposable emulator. CI does not have one, so it only runs the
non-ignored tests.

CI ([.github/workflows/ci.yml](.github/workflows/ci.yml)) runs `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings`, `cargo build`, and `cargo test` on macOS and
Linux.

## Platform support

- **macOS** is the development target and the only platform the app has been used on.
- **Linux** is in the CI matrix and green, running the logic tests. GPUI's Linux stack needs the
  ALSA, fontconfig, GLib, Vulkan-loader, Wayland and X11 dev libraries that
  [ci.yml](.github/workflows/ci.yml) installs. A debug build of this dependency graph reaches
  12 GB, which is more than a fresh runner has free, so the job reclaims the image's unused
  toolchains before building — see ROADMAP M5 if you are copying this workflow.
- **Windows** is not supported yet — see ROADMAP M5.

## Documentation

- [CONSOLE_V2_SPEC.md](CONSOLE_V2_SPEC.md) — what the console is, service by service, with
  ✅ implemented vs 🧭 planned on each claim.
- [ROADMAP.md](ROADMAP.md) — what is still missing, measured against the code and the running
  emulator.
- [SERVICES.md](SERVICES.md) — all 364 catalog services and whether a page manages each one.
- [CHANGELOG.md](CHANGELOG.md) — release history.

## License

Apache-2.0. See [LICENSE](LICENSE). The dependency licences that matter for this choice:
`gpui-kit`/`gpui-pre` and every `aws-sdk-*`/`aws-config` crate are Apache-2.0; `tokio` and
`axum` are MIT, which is compatible.

## Contributing

The scope rule for this project: **everything the emulator provides must be manageable from
this dashboard.** A service with no driver is not a stub to ignore — it is a gap in ROADMAP M1.
When adding a service, follow the pattern the thirteen existing drivers set: one `aws-sdk-*`
crate in `Cargo.toml`, one `src/aws/drivers/<name>.rs`, one `MANAGED` entry in
[src/views/catalog.rs:11](src/views/catalog.rs#L11), and rows in
[src/views/operations.rs](src/views/operations.rs). Do not hand-roll SigV4 or Smithy wire
code — use the AWS SDK.
