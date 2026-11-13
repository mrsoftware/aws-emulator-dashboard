# Changelog

Notable changes to the AWS Emulator Dashboard. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres to
[semantic versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- **Linux CI is no longer red at the lint step.** The first two pushes failed
  `cargo clippy --all-targets` on `ubuntu-latest` with `E0433: cannot find AppleTextSystem in
  gpui_apple`: the headless render module reached for CoreText on a platform that has no such
  crate. The module is now gated to macOS ([src/main.rs:7](src/main.rs#L7)) and the crate with it.
  The 11 page-screenshot tests run on macOS only; the logic tests still run on both.
- **Linux CI no longer dies in the link step.** Once clippy passed, `cargo test` on
  `ubuntu-latest` failed with `linking with cc failed`, and the log named the victim: rust-lld
  crashed with `collect2: fatal error: ld terminated with signal 7 [Bus error]` while linking the
  test target, after `cargo build` had linked the same crate graph on the same disk. It was
  runner disk, matching GitHub's triage for this signature (actions/runner-images#13367): the job
  starts with 15G free and `target/` reaches 12G. The workflow now deletes the .NET, Android, GHC
  and CodeQL payloads the image ships, which leaves 37G free before the build. Two earlier
  theories were wrong and are undone: the Mesa, Vulkan, EGL, GBM and DRM dev packages bought
  nothing — a per-library probe showed all twenty link fine — and the crash survived removing the
  GPU stack from the test binary.
- **Linux CI no longer links a GPU stack it cannot use.** `gpui-pre` and `gpui-pre-platform` were
  unconditional dev-dependencies with `test-support`, which unifies GPUI's features across the
  graph: `gpui-pre-platform` maps that feature to `gpui_wgpu/test-support`, and `gpui_wgpu` is a
  Linux-target dependency. So Linux rebuilt `gpui-pre`, `gpui-pre-wgpu`, `gpui-pre-linux`,
  `gpui-kit` and `proptest` and linked a second copy of all of them into a test binary containing
  no render test. All three GPUI test crates are now macOS-only dev-dependencies
  ([Cargo.toml:43](Cargo.toml#L43)). This is safe on Linux because
  `TestSupportExt::test_support`, which the views call on every platform, returns `Self`
  unchanged without the feature.

### Verification status

`a2fc867` is the first commit green on both CI legs: macOS 25 tests, Linux 14 passed and 23
ignored, which is what the platform split predicts. macOS was green throughout.

## [0.1.0] — 2026-11-13

First release. A native Rust + GPUI desktop console for a local AWS emulator.

### Added

- **Console shell.** AWS-console-style layout: sidebar with FAVORITES / EMULATOR / ALL
  SERVICES sections and collapsible category groups, page header, and a footer status strip.
  Both the page body and the sidebar scroll independently ([src/views/root.rs:555](src/views/root.rs#L555)).
- **Design system.** A single palette and set of builders — cards, panels, page headers,
  section labels, pills, dots, tiles, tables, chips — in [src/views/theme.rs](src/views/theme.rs).
  No view hardcodes a colour.
- **Resource pages** for the eleven services the emulator serves and the dashboard can manage:
  S3, DynamoDB, SQS, EC2, IAM, SNS, RDS, CloudWatch, CloudWatch Logs, Kinesis, and Secrets
  Manager. Each has a resource table and a per-resource operations card.
- **Operations card.** 78 operations across those services, read-only ones streaming output
  into the panel and write ones running against the emulator, with typed dialogs and
  pre-submit duplicate checks.
- **Instances page.** EC2 table with state, type, IPv4 and launch time; launch, start, stop and
  terminate, each with a state-appropriate affordance and a colour-coded state dot.
- **All-services catalog.** The 364-service baked catalog in [assets/services.json](assets/services.json),
  grouped into 17 categories, searchable, with both grid and list views and an explicit
  "served, no page" marker for a service the emulator answers but this build cannot manage.
- **Emulator control page.** Start and stop the emulator through Docker or a local binary,
  edit port, auth, telemetry, and idle timeout, and adopt an emulator that was already running
  when the dashboard opened.
- **Traffic log.** When the optional reverse proxy is enabled, every request the dashboard sent
  is listed with method, path, status, and latency. The log holds no headers, so a signed
  request's credentials never land in it.
- **Settings.** Eight keys persisted as TOML in the platform config directory
  ([src/settings.rs](src/settings.rs)); missing or unparsable files fall back to defaults.
- **Secret handling.** A one-time secret (Secrets Manager value, IAM access key secret) is
  revealed in the UI and shown in a dismissable banner; it is never written to a log, a file,
  or a test output.
- **Tests.** 25 unit and headless-render tests that CI runs, plus 25 live-emulator tests
  marked `#[ignore]`. The render tests include the scroll regression that proves a page
  overflows in a short window.
- **CI.** `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo build`, and
  `cargo test` on macOS and Linux ([.github/workflows/ci.yml](.github/workflows/ci.yml)).
- **Docs.** [README.md](README.md), [CONSOLE_V2_SPEC.md](CONSOLE_V2_SPEC.md),
  [ROADMAP.md](ROADMAP.md), [SERVICES.md](SERVICES.md), and Apache-2.0 `LICENSE`.

### Known gaps

- 327 served services have no driver, so nothing can be managed through them — ROADMAP M1.
- No service icons; every tile is a monogram, and two services can collide on one.
- The revealed secret has no Copy button, and no dialog closes on `Escape`.
- Live-emulator tests never run in CI, because CI has no emulator container.

[Unreleased]: https://github.com/mrsoftware/aws-emulator-dashboard/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/mrsoftware/aws-emulator-dashboard/releases/tag/v0.1.0
