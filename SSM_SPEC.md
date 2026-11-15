# SSM_SPEC.md — AWS Systems Manager (Parameter Store) in the dashboard

Status: ✅ implemented (2026-11-14), with one deviation from this spec: the
version history is the detail panel's own children table, refreshed by the same
Refresh button every page has, rather than a separate History button — the IAM
access-keys pattern. The reveal banner, create dialog and delete are as specced.

Scope: **dashboard only**. The emulator is never edited. Its SSM crate serves 154
operations, of which the Parameter Store surface is fully implemented — verified in
`services/service-com-amazonaws-ssm-amazonssm/src/ops/`: `PutParameter` (870 lines),
`GetParameter` (393), `GetParametersByPath` (492), `DescribeParameters` (411),
`GetParameterHistory` (375), `DeleteParameter`, `DeleteParameters`,
`LabelParameterVersion`. The rest of SSM (Run Command, maintenance windows, patch
baselines, OpsItems) is mostly 501 stubs — the page is **Parameter Store only** and
says so; it does not pretend to manage instances.

## Design

### Resource list

`ssm::fetch(config)` → `Vec<Resource>` via paginated `DescribeParameters`
(`aws-sdk-ssm`, new `Cargo.toml` entry). Row: `name = parameter name`,
`detail = "<type> · modified <date>"`. No tier shown in v1.

### Create: `Dialog::CreateParameter`

Three field rows reusing existing dialog entities — no new state:

| field | entity | hint |
|---|---|---|
| Name | `input_name` | e.g. `/app/db/url` |
| Value | `input_json` | text, or comma-separated for StringList |
| Type | `input_protocol` | `String`, `StringList` or `SecureString`; empty = `String` |

Backed by `PutParameter` with `Overwrite=false`; the create path pre-checks the
listing so an existing name is refused before the wire call (the secrets-manager
precedent, [detail.rs:699](src/views/detail.rs#L699)). `SecureString` is created
without `KeyId` — the emulator seals it under the default key
([state.rs:388](../aws-emulator/services/service-com-amazonaws-ssm-amazonssm/src/state.rs#L388)).

### Detail card buttons

1. **Reveal value** — `GetParameter` with `WithDecryption=true`. A `SecureString`
   decrypts only if the emulator's KMS gate permits the caller `kms:Decrypt`; when
   it refuses, that refusal is what the user reads — no faked plaintext. The value
   lands in `detail.revealed` (one-time banner, the access-key precedent) and is
   never stored or logged.
2. **History** — `GetParameterHistory` rendered as the children block: one row per
   version (`name = version <n>[ <label>]`, `detail = modified <date>`).
3. **Refresh**.
4. **Delete parameter** — `DeleteParameter`; wired into `delete_resource`.

### Catalog wiring

- `MANAGED` gains `("ssm", crate::aws::drivers::ssm::LABEL)` — 13 entries.
- `create_action("ssm")` → `("New parameter", Dialog::CreateParameter)`.
- `resource_noun("ssm")` → `"parameter"`.

### Files touched

| File | Change |
|---|---|
| `Cargo.toml` | + `aws-sdk-ssm = "1"` |
| `src/aws/drivers/ssm.rs` | new: `LABEL`, `fetch`, `create`, `reveal`, `history`, `delete` |
| `src/aws/drivers/mod.rs` | + `pub mod ssm;` |
| `src/views/catalog.rs` | MANAGED entry, create_action, resource_noun |
| `src/views/root.rs` | fetch/create/delete dispatch; wiring-test block |
| `src/views/detail.rs` | `Dialog::CreateParameter` fields/validation/buttons; reveal + history actions |
| `src/app_state.rs` | `Dialog::CreateParameter` variant |
| `tools/gen-services-doc.py` | DRIVER/LABEL/NOUN entries |
| `SERVICES.md`, `ROADMAP.md`, `README.md`, `CONSOLE_V2_SPEC.md` | counts 12 → 13, 1,642 → 1,794 ops |

### Honest gaps

- 🧭 **96 of 154 SSM operations are stubs** in the emulator (Run Command,
  maintenance windows, associations, OpsItems). The page's focus panel covers
  Parameter Store only.
- 🧭 **SecureString reveal** is conditional on the KMS gate, by the emulator's own
  design — the test asserts the String path; a SecureString live check asserts the
  wire call succeeds and leaves the ciphertext/plaintext question to the gate.
- 🧭 **No labels, tags, policies or tiers in v1** — `LabelParameterVersion` is
  served but gets no button; `Policies` and `Tier` stay API-only.
- 🧭 **No hierarchical browsing** (`GetParametersByPath` is served but the page
  lists all parameters flat, like DescribeParameters does).
- 🧭 **No batch put/get** — one parameter per action, like every other page.

### Test plan

1. Render tests: STS-style — parameters page empty and with rows + reveal banner
   (`ssm-parameters-empty`, `ssm-parameters-reveal`).
2. Live tier A block: create a parameter (`dash-wiring-ssm-<nanos>`, type String),
   assert it lists, reveal its value equals what was put, read its history
   (one version), delete, assert absent.
3. Negative: reveal of an absent parameter errors with the emulator's
   `ParameterNotFound` wording; create with an existing name is refused by the
   pre-check.
4. Wiring test additions: `create_resource("ssm", …)` IS wired (unlike sts) —
   covered by the page! block, not the refused() table.
