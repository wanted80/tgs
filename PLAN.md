# tgs implementation and release plan

## Objective and success criteria

Build `tgs`, a production-grade, cross-platform command-line program that sends text notifications through Telegram. The core release succeeds when it can securely manage named profiles, accept exactly one message source, split and send plain text safely, expose stable machine-readable results and documented exit codes, and pass offline quality gates on Linux, macOS, and Windows.

## Locked product and technical decisions

- The executable is named `tgs`; `tgnotify` is the preferred alternative if renamed before publication.
- The repository uses Rust 2024 with an explicit MSRV and committed `Cargo.lock`.
- `main.rs` handles process I/O and exit status only. CLI parsing, configuration, input, splitting, transport, and output live in a small library.
- Runtime dependencies are limited to `ureq` (Rustls and JSON), `serde`, `serde_json`, `rpassword`, `hostname`, `getrandom` (secure pairing), `qrcode` (terminal QR rendering without image features), and `fs2` (OS file locking on the Rust 1.85 MSRV). Dependencies must be pinned by Cargo and justified.
- The implementation is blocking and reuses one HTTP agent with enforced timeouts. It does not use async Rust, `curl`, media, plugins, or speculative extension points.
- Syntax is `tgs [send] [OPTIONS] [MESSAGE...]`; `send` is optional. A message comes from exactly one of positional words, `--message`, `--file`, or implicit piped stdin. An interactive invocation without a message prints help and exits.
- Send configuration includes profiles, destination override, parse mode, notification flags, topic, timeout, chunk cap, and silent/text/JSON output. `--verbose` aliases text output.
- Configuration is versioned JSON with a default profile and named token/chat profiles. Native platform paths, documented environment overrides, atomic same-directory writes, and restrictive Unix permissions are mandatory.
- Tokens are accepted only from hidden interactive input, `--token-stdin`, configuration, or `TGS_TOKEN`. They are never command-line send arguments, shown by `config show`, logged, or included in errors. Setup asks before persisting an environment token.
- Plain text is split above 4096 Unicode scalar values, preferring newline boundaries. Oversized HTML and MarkdownV2 are rejected. The chunk cap is checked before network activity.
- Telegram requests are typed JSON `sendMessage` calls. Chunks are sequential. Only HTTP 429 is retried, at most twice, honoring `retry_after` up to 60 seconds. Timeouts, transport errors, 5xx, and ambiguous replies are not retried.
- Exit codes are 0 success, 2 usage/input, 3 configuration/filesystem, and 4 Telegram/network/retry/partial-delivery.
- Structured success and failure output is schema-versioned and includes status, profile, destination, chunks delivered, and Telegram message IDs, but never tokens.
- The core scope remains text only. Command execution, attachments, keyboards, rich messages, and OS credential stores are deferred.
- The existing `telegram-sender` repository remains unchanged and no GitHub remote is created initially.

## Interfaces

Send options: `--profile NAME`, `--chat-id ID`, `--config PATH`, `--parse-mode plain|html|markdown-v2`, `--silent`, `--no-preview`, `--protect`, `--topic ID`, `--timeout SECONDS` (default 10), `--max-chunks N` (default 10), `--output silent|text|json` (default silent), and `--verbose`.

Configuration commands:

- `tgs config init [PROFILE]`
- `tgs config set PROFILE [--chat-id ID] [--token-stdin]`
- `tgs config list`
- `tgs config show [PROFILE]`
- `tgs config use PROFILE`
- `tgs config remove PROFILE`
- `tgs config path`

Precedence:

1. Configuration path: `--config`, `TGS_CONFIG`, platform default.
2. Profile: `--profile`, `TGS_PROFILE`, configured default, only configured profile.
3. Credentials: `TGS_TOKEN` / `TGS_CHAT_ID`, selected profile.
4. `--chat-id` overrides only the destination; explicit send options override defaults.

## Milestones

- [x] M0: Initialize local Git repository and record this plan.
- [x] M1: Add engineering guidelines and repository/legal scaffolding.
- [x] M2: Implement CLI, configuration, message validation/splitting, and output models.
- [x] M3: Implement Telegram transport, retry policy, and partial-delivery reporting.
- [x] M4: Add unit, integration, and end-to-end tests with no external network dependency.
- [x] M5: Add documentation, CI, release packaging, and complete all quality gates.
- [x] M6: Add friendly `bot` profile management, live `getMe` validation, crate
  publication metadata, centralized lints, package validation, dependency updates,
  and read-only vulnerability auditing.
- [x] M7: Harden Windows subprocess E2E networking and isolate package verification
  from Rust cache cleanup.
- [x] M8: Add automatic, format-safe source and device labels to every sent message,
  with explicit opt-out, deterministic overrides, and hostname privacy guidance.
- [x] M9: Complete the public-release readiness pass: remove version/help drift,
  strengthen preflight validation and configuration invariants, improve onboarding
  documentation, and add architecture and adversarial regression tests.
- [x] M10: Fix secret-bearing configuration errors, panic-prone process I/O and
  timeout values, bounded input/splitting, and transport test shutdown.
- [x] M11: Add standalone `setup` with private/group chat pairing, destination
  confirmation, atomic saving, and an optional test notification; add `status`,
  implicit single-profile/environment selection, and friendly terminal errors.
- [x] M12: Cover complete command workflows with injected I/O and local HTTP,
  update documentation, and pass the Docker quality suite and MSRV check.
- [x] M13: Fix the final-review findings: fail-closed environment decoding,
  whitespace-only message chunks, successful-response redaction, concurrent
  configuration updates, incompatible pairing filters, leading-hyphen profile
  names, numeric destination overflow, and unbounded timeout-test fixtures.
- [x] M14: Promote reproductions to permanent regression tests, document behavior
  changes, and rerun Docker quality gates and Rust 1.85 compatibility.
- [x] M15: Fix the Rust 1.85 Clippy failures reported by GitHub CI, add focused
  pairing regressions, and run all five quality gates on both Docker toolchains.
- [x] M16: Fix the September review findings with permanent regressions:
  setup follow-up routing, Telegram-normalized blank chunks, consistent profile
  selection, and feasible long-whitespace splits.
- [x] M17: Simplify the main help and guided destination selection, show setting
  origins in status, document the changes, and verify both Docker toolchains.
- [x] M18: Make GitHub Releases compatible with immutable asset publication by
  uploading all binaries to a tagged draft before publishing the release.

## Test and release acceptance criteria

- Unit tests cover CLI parsing, precedence, profiles, paths, permissions, redaction, validation, exit codes, output schema, exact split limits, newline preference, emoji and multibyte text, long lines, chunk caps, and formatted-message rejection.
- A local standard-library HTTP server plus an injectable sleeper covers success, API errors, malformed replies, timeout behavior, 429 behavior, no-retry cases, and partial delivery.
- End-to-end tests cover positional, option, file, and stdin input; profiles; all output modes; and configuration commands.
- Normal tests require neither a Telegram token nor external network access.
- `cargo fmt --check`, `cargo check`, Clippy with warnings denied, all tests, and release builds pass.
- CI runs the quality gates on Linux, macOS, and Windows.
- Tag releases produce checksummed archives for Linux x86_64/ARM64, macOS Intel/Apple Silicon, and Windows x86_64.
- Documentation covers installation, BotFather setup, chat discovery, cron, pipelines, coding-agent use, profiles, security, metadata, exit codes, and long-message behavior.

## Decision log

- 2026-09-10: `v0.4.0` exposed that repository release immutability is enabled
  while the existing pipeline publishes through Release Please before the
  tag-triggered binary matrix runs. GitHub sealed the empty release, so all
  five uploads received HTTP 422 and the release has no assets. M18 changes
  Release Please to create a tagged draft, keeps the matrix upload unchanged,
  and publishes only after every target succeeds. A failed matrix leaves the
  draft available for diagnosis and retry.

- 2026-09-10: M18 complete. Release Please now uses `draft` plus
  `force-tag-creation`, and the tag workflow publishes only after all five
  binary uploads succeed. JSON, diff, and the pinned Rust 1.98 Docker gates
  pass: formatting, check, strict Clippy, 149 tests, and release build.

- 2026-09-10: The user authorized committing the completed M16/M17 changes,
  pushing `feat/setup-and-hardening`, and opening a pull request against `main`.
  This supersedes the earlier publication hold; implementation scope is
  unchanged. Include the offline Docker verification and compatibility notes
  in the PR, and leave merging and live Telegram testing outside this delivery.
- 2026-09-10: M17 complete. Guided setup now offers private/group/channel
  destinations, status explains setting origins, and help is split into a short
  overview and focused command pages. All 149 tests and all five quality gates
  pass in offline Docker on Rust 1.98 and 1.85. Rustdoc with warnings denied,
  package verification (26 files), and release-binary help smoke checks pass on
  1.98. No dependencies or lockfile changed; cargo-audit was not rerun. Existing
  commands and saved profiles remain supported without migration. No protocol
  change, live Telegram calls, commits, or pushes were made; native-platform
  verification of these changes awaits a future GitHub CI run.
- 2026-09-10: M16 implemented and the initial expanded suite passes all 146
  tests in offline Docker. Reject unsupported message controls (including NUL
  and ANSI escapes) rather than silently stripping them; keep tabs/newlines/CR.
  Classify Telegram-normalized invisible characters when finding safe chunk
  boundaries, preserving all accepted bytes. A bounded fallback relaxes newline
  preference, and an independent exhaustive partition oracle verifies acceptance
  as well as output invariants. Profile selection is shared, saved-token queries
  are labeled, and setup's follow-up command is tested against active overrides.
  M17 implementation is present; final coverage, documentation, and both full
  toolchain gates are still in progress.
- 2026-09-10: The user authorized the remaining fixes and usability changes on
  the existing branch. Keep the standalone HTTPS Bot API, BotFather token flow,
  Rust 1.85 MSRV, existing commands, and configuration schema. Do not introduce
  hosted bot management, new dependencies, commits, pushes, or live Telegram
  calls. Make setup's suggested command honor its confirmed profile/destination
  and warn about environment overrides without printing their values. Share
  profile selection with read-only bot/config commands while keeping their
  credential view explicitly local. Preflight Telegram-normalized blank text
  and preserve feasible whitespace distributions. Offer private/group/channel
  choices in interactive setup, add focused help, and include non-secret setting
  origins in status as additive schema-v1 fields.
- 2026-09-10: Review baseline: all 134 existing tests and all five quality gates
  pass in offline Docker on Rust 1.98 and 1.85; rustdoc and package checks pass.
  Four isolated review probes reproduce the remaining issues on both versions.
  Commit `3c069a1` is now on origin and all four GitHub quality jobs passed in
  run 33992807700. The earlier pending-push note is obsolete.
- 2026-09-05: Inspect CI run 33992067940 with `gh`. Stable Linux, macOS, and
  Windows jobs passed; Rust 1.85 failed Clippy before running its tests.
  `format_collect` rejects per-byte formatting of the pairing challenge, and
  `nonminimal_bool` rejects the inverted optional-human-sender check. The
  previous local MSRV verification covered check/tests, not MSRV Clippy.
  Preserve behavior and strict CI settings: append hexadecimal bytes directly
  to one string, simplify the sender predicate, and cover both with focused
  regressions. Verify the full five-gate suite with Rust 1.85 as well as 1.98.
  The user authorized committing this follow-up after verification, but not
  pushing it; the earlier commits are now on the remote branch.
- 2026-09-05: M15 complete. Reproduced both Rust 1.85 Clippy errors in Docker
  before the fix. The complete five-gate suite now passes on Rust 1.85.0 and
  Rust 1.98, including strict Clippy and all 134 tests. New regressions verify
  fixed-width lowercase challenge encoding with leading zeroes and the
  missing/null/bot/human sender cases. Rustdoc and 25-file package verification
  also pass on 1.98. No CI checks were disabled, no dependencies changed, and
  no live Telegram requests were made. The fix is recorded in a local commit;
  GitHub verification of the follow-up awaits an authorized push.
- 2026-09-05: The user authorized local commits of the completed setup and
  review fixes. Reran all five required quality gates in offline Rust 1.98
  Docker; all 132 tests passed. Implementation and regression tests are in
  feature commit `20fa9a9`; the accompanying documentation commit records the
  setup guide, changelog, and verification notes. Pushing, publishing, and
  live Telegram checks remain outside this authorization.
- 2026-09-05: Begin the authorized corrective pass on the existing branch. Keep
  schemas, HTTPS Bot API, and the Rust 1.85 MSRV unchanged. Decode selected
  environment values fallibly while preserving explicit overrides. Preflight
  every plain-text chunk so blank tails cannot fail after delivery. Redact
  successful Telegram metadata at the transport boundary. Protect configuration
  comparison and replacement with a short OS-backed lock; never hold a lock
  across prompts or network requests. Inspect pairing filters without changing
  them. Support `--` for option-like profile names, and bound numeric IDs to i64.
  Test fixtures must shut down explicitly even when no request arrives.
- 2026-09-05: Use `fs2` 0.4.3 for nonblocking, OS-released advisory locks; the
  equivalent standard-library API requires Rust 1.89. A persistent empty
  `.tgs-config.lock` file serializes saves in each configuration directory.
  All CLI mutations compare their original snapshot under the lock before
  atomic replacement. Conflicting edits fail with an actionable retry error.
  Do not unlink the lock file: that would let concurrent processes lock
  different inodes. Direct editing by non-cooperating programs is not locked.
- 2026-09-05: M13 implemented and 129 permanent tests pass with strict Clippy
  in Docker. Plain-text splitting preserves input and rebalances short blank
  tails; unsendable blank chunks are rejected before delivery. All selected
  environment values retain native encoding until fallible lookup. Successful
  Telegram metadata is redacted at the transport boundary. Timeout fixtures
  now cancel missing/incomplete requests and verify the actual timeout category.
  M14 remains open for final quality, MSRV, package, and dependency checks.
- 2026-09-05: Final parallel verification exposed that closing a lock handle
  alone can leave a Unix lock briefly inherited by a concurrent child before
  exec. Explicitly unlock in the guard's destructor before closing; a duplicated
  handle regression verifies immediate release without sleeps. Reject reserved
  lock paths on load as well as save, before setup prompts or network requests.
- 2026-09-05: M14 complete. All 132 permanent tests pass in offline Docker on
  Rust 1.98 and the Rust 1.85 MSRV. The CLI integration suite passed ten repeated
  runs, including cross-process configuration locking; all nine original
  isolated review probes also pass against the final source. Formatting,
  locked all-target/all-feature check, strict Clippy, release build,
  warning-free rustdoc, and verification of the 25-file package pass.
  Cargo-audit 0.22.2 reports no vulnerabilities for the 99 locked dependencies
  against 1,239 RustSec advisories. No live Telegram requests, native
  macOS/Windows runtime checks, commits, pushes, or publication were performed.
- 2026-09-05: Start `feat/setup-and-hardening` for the authorized review fixes and
  standalone setup wizard. Keep HTTPS Bot API and local token ownership. Hosted
  managed-bot onboarding, shared relays, and MTProto remain separate proposals.
  Setup will display a Telegram pairing link (with optional QR), use a random
  single-use challenge, explicitly confirm the destination, and save only the
  finished profile. Existing webhooks/other update consumers must not be reset
  or have updates acknowledged by the wizard. Status will inspect configuration
  and live identity/access without sending a notification.
- 2026-09-05: Move command orchestration behind injectable library I/O so actual
  success output and wizard flows can be exercised offline without a production
  endpoint override. Preserve JSON errors for redirected stderr and explicit
  JSON output; use friendly errors on terminals. Bound requests to 1-300 seconds
  and input to a documented 16 MiB ceiling plus the selected chunk budget.
- 2026-09-05: M10-M11 implemented. Setup pairs on an explicit Enter press after
  Start in Telegram, so retries require user action and no background polling
  service is needed. It never sends update offsets or resets webhooks/filters;
  a busy bot can use an explicit destination. Optional QR uses compact Unicode
  rendering. Setup detects configuration edits during its prompts before saving.
  The expanded Docker suite passed 107 tests before final documentation and
  boundary refinements; final quality/MSRV verification is M12.
- 2026-09-05: M12 complete. All 109 deterministic tests pass in Docker on Rust
  1.98 and the Rust 1.85 MSRV. Formatting, locked check, strict all-target and
  all-feature Clippy, release build, warning-free rustdoc, and package
  verification pass. Terminal smoke checks confirm readable errors and explicit
  JSON errors. Cargo-audit 0.22.2 reports no vulnerabilities against 1,239
  advisories. No live Telegram requests were made; native macOS/Windows runtime
  verification remains covered by CI, not this local Linux Docker run.

- 2026-08-26: Adopted `tgs` as the executable name and a small synchronous Rust library architecture.
- 2026-08-26: Chose JSON configuration and Rustls-backed `ureq` to provide native cross-platform HTTPS without external executables.
- 2026-08-26: Limited automatic retries to explicit 429 replies to avoid duplicate messages after ambiguous failures.
- 2026-08-26: Added a pinned Rust 1.98 dev container and moved all Rust development and verification into Docker at the user's request; Rust 1.85 remains the declared MSRV and is verified separately.
- 2026-08-26: Replaced the tag workflow's release creator with SHA-pinned Release Please (Rust strategy) so Conventional Commits drive release PRs, changelogs, version bumps, and tags; binary uploads remain in a separate tag-triggered workflow.
- 2026-08-26: Audited upstream action tags and pinned all CI/release actions to immutable full commit SHAs, with Dependabot configured to propose future action updates.
- 2026-08-26: Removed platform-specific needless `return` statements rejected by current stable Clippy; Release Please's token has the declared write scopes but GitHub still rejects PR creation, so repository-level Actions policy must be checked before the first release.
- 2026-08-27: Kept Telegram bot creation in BotFather and defined `tgs bot` as a
  friendly schema-v1 profile workflow. Managed-bot deep links remain deferred
  because they require hosted update handling and token custody.
- 2026-08-27: Chose typed, injectable `getMe` validation before bot-profile writes;
  `main.rs` remains responsible only for hidden/stdin token input and process I/O.
- 2026-08-27: Centralized a balanced Rust, rustdoc, and Clippy lint baseline in
  `Cargo.toml`; broad pedantic, nursery, cargo, and restriction groups remain out
  of scope.
- 2026-08-27: Added weekly Cargo dependency updates and a minimum-permission
  `cargo-audit` 0.22.2 workflow. The first local audit found RUSTSEC-2026-0009;
  the inactive transitive `time` lockfile entry was updated to 0.3.47 while the
  active dependency graph continued to pass Rust 1.85 checks and Clippy.
- 2026-08-27: Preserved Windows `SystemRoot` after clearing the E2E child
  environment so local HTTP socket initialization remains functional. Moved
  `cargo package` verification to a temporary target directory so rust-cache
  does not inspect Cargo's extracted package tests as cache test artifacts.
- 2026-08-27: Chose per-invocation message labels in the form
  `[source · device]`, defaulting to source `tgs` and the locally detected
  hostname. IP discovery is intentionally excluded; invalid or unavailable
  hostnames fall back to `unknown-device`, and `--no-label` preserves the
  original message while ignoring identity environment variables.
- 2026-08-27: Started a final public-release audit as M9. The pass stays within
  the existing synchronous, text-only architecture and prioritizes CLI clarity,
  validation before side effects, configuration integrity, secret safety, and
  durable structural tests over new product features.
- 2026-08-27: Replaced the manual token-bearing `getUpdates` onboarding step
  with a read-only `tgs bot chats` lookup. It uses short polling without an
  offset, so Telegram updates remain unconfirmed, and keeps token handling
  inside the existing typed blocking transport.
- 2026-08-27: Moved configuration mutations out of `main.rs`, added structural
  tests for the binary/library boundary, and removed the debug-build runtime
  endpoint override. A targeted `cargo-mutants` 27.1.0 release run against the
  splitting module exposed three assertion gaps; focused boundary tests now
  catch all 47 viable mutants (one additional generated mutant is unviable).
  Mutation testing remains a documented periodic release tool rather than a
  slow per-commit CI gate.
- 2026-08-27: Unified token/chat validation across setup, persisted
  configuration, and sends; made configuration/environment debug output
  secret-safe; and switched replacement to collision-safe `create_new`
  temporaries plus cross-platform overwrite rename without a Windows delete gap.

## Resume here

- Current state: M0-M18 complete on `feat/setup-and-hardening`. The final fixes
  follow `3c069a1` and are approved for commit, push, and a pull request to `main`.
- Last verification: September 10 final checks pass all five gates and all 149
  tests on both offline Docker toolchains (Rust 1.98 and 1.85). Documentation,
  package verification (26 files), and release-binary help smoke checks pass on
  1.98. The four review findings have permanent regression coverage.
- Next action: deliver the M18 workflow fix, then confirm a future tag creates a
  draft, uploads all five archives, and publishes only after the matrix
  succeeds. Merging and live Telegram checks require separate authorization.
  BotFather remains part of standalone setup, and saved profiles need no
  migration. Dependencies are unchanged; no fresh cargo-audit was run.
- Safety note: never place credentials in this file or repository history.
