# STS_SPEC.md — AWS Security Token Service in the dashboard

Status: ✅ implemented (2026-11-14). The assume-role workbench shipped as specced; the
`MintedSession` record also carries the minted access key id, shown once in the reveal
banner and never stored beyond it. See the gaps list below for what v1 does not cover.

Scope: **dashboard only**. The emulator is never edited for this feature —
its STS crate already serves 10 of 11 operations; the one stub
(`AssumeRoot`, `services/service-com-amazonaws-sts-.../src/ops/assume_root.rs:203`)
stays a 501 and is listed as a gap, not fixed here.

## Where STS stands today

| Surface | State |
|---|---|
| Emulator STS crate | ✅ 10/11 ops implemented (`AssumeRole`, `GetSessionToken`, `GetFederationToken`, `AssumeRoleWithWebIdentity`, `GetCallerIdentity`, `GetAccessKeyInfo`, `DecodeAuthorizationMessage`, `GetDelegatedAccessToken`, `GetWebIdentityToken`); 🚧 `AssumeRoot` stub |
| `assets/services.json` | ✅ entry present: `signingName: "sts"`, `registered: true`, 11 ops, category Security & Identity |
| `src/views/catalog.rs` `MANAGED` | ❌ absent — STS renders as one of the "not manageable yet" rows ([root.rs:778](src/views/root.rs#L778)) |
| `src/aws/drivers/sts.rs` | ❌ absent |
| `SERVICES.md` | 🚧 at line 144 |

## The constraint that shapes the design

STS has **no list operation**. Dashboard drivers reach the emulator through
AWS SDK clients ([drivers/iam.rs:9](src/aws/drivers/iam.rs#L9)), so the STS
page cannot fetch emulator-held sessions over the wire. What the page *can*
honestly show:

1. **Roles** — fetched with the IAM SDK client the dashboard already carries
   (`iam::fetch_roles`, [drivers/iam.rs:48](src/aws/drivers/iam.rs#L48)). The
   resource list of the STS page is the account's assumable roles.
2. **Mints** — sessions the dashboard itself minted this run, verified by
   calling `GetCallerIdentity` *with the minted credentials*. That call is the
   verification, not decoration: it proves the credential the emulator vended
   actually signs and resolves to the assumed identity.

## Design: assume-role workbench

### Resource list

`sts::fetch(config)` → `Vec<Resource>` where each row is one role:
`name = role name`, `detail = role ARN`. Backed by `iam::fetch_roles` (reuse,
no duplicate ListRoles loop — [collapse-duplication rule]).

### The action: Assume

Per-row **Assume** button on a role row (the IAM precedent: access keys are a
row action, not a dialog — [detail.rs:874](src/views/detail.rs#L874)).

1. `sts::assume(config, role_arn, session_name)` calls
   `aws_sdk_sts::Client::assume_role()` with `RoleArn`, `RoleSessionName`
   (default `"dashboard"`, 2..64 chars per the model), duration left unset
   (emulator default 3600).
2. On success, immediately build a second STS client whose credentials are the
   minted `AccessKeyId`/`SecretAccessKey`/`SessionToken` and call
   `GetCallerIdentity`. Display the returned ARN/user id — that is the proof
   the session works.
3. Record the mint in dashboard-side state.

### Dashboard-side session state

Stored per mint (bare answer struct — no wrappers, no request copies):

```rust
struct MintedSession {
    role_name: String,     // what the row was
    session_name: String,  // RoleSessionName sent
    identity_arn: String,  // GetCallerIdentity answer
    expires_at: String,    // formatted credential expiry
}
```

No secret material is stored: the workbench verifies a mint, it is not a
keychain. Secret keys are shown once in the result area and then dropped.

### Catalog wiring

- `MANAGED` gains `("sts", crate::aws::drivers::sts::LABEL)` — 12 entries.
- `create_action("sts")` → `None`; `resource_noun("sts")` → `"session"`.
  The manageability test at [root.rs:1357](src/views/root.rs#L1357) wants a
  "way to make one" — the Assume row action is that way; if the check demands
  a `create_action` entry, the row action is registered there instead and the
  dialog collects only the session name.
- `Dialog::AssumeRole` variant only if the row-action path needs a prompted
  session name; otherwise no new Dialog variant.

### Files touched

| File | Change |
|---|---|
| `Cargo.toml` | + `aws-sdk-sts = "1"` |
| `src/aws/drivers/sts.rs` | new: `LABEL`, `fetch`, `assume`, `verify` |
| `src/aws/drivers/mod.rs` | + `pub mod sts;` |
| `src/views/catalog.rs` | MANAGED entry, `resource_noun` |
| `src/views/detail.rs` | page wiring: fetch key `"sts"`, row action |
| `src/app_state.rs` | `minted_sessions: Vec<MintedSession>` (only if page-local state is insufficient) |
| `SERVICES.md` | row 144 🚧 → ✅ managed |
| `src/console_render_tests.rs` / live tests | render test for the STS page; live test minting against a running emulator |

### Honest gaps

- 🧭 **AssumeRoot** stays a 501 in the emulator (out of scope by decision).
- 🧭 **MFA-protected assume** is untestable end-to-end: the emulator's MFA
  challenge always fails for a complete challenge (only FIDO security keys are
  held; `mfa_challenge_failure` in the STS state can never succeed). The page
  sends no MFA fields.
- 🧭 **Session policies / tags** are not exposed in the workbench v1 —
  `AssumeRole` accepts them over the wire but the page sends none.
- 🧭 `GetSessionToken` / `GetFederationToken` mints are not surfaced; only
  role assumption. The emulator serves both, but the workbench's resource
  list is roles, and a federated mint has no role row to hang off.
- The minted-session list is dashboard-local: restarting the dashboard (or
  another client minting) shows nothing. That is the honest consequence of
  STS having no list op.

### Test plan

1. Render test: STS page draws with zero roles and with roles (macOS-only like
   the other 11 render tests).
2. Live test (`#[ignore]`): against a running emulator — list roles, assume
   the IAM-created role, assert GetCallerIdentity ARN matches
   `arn:aws:sts::<account>:assumed-role/<role>/<session>`.
3. Negative: assume a role the account does not hold → the emulator's
   not-authorized wording surfaces in `action_error`.
