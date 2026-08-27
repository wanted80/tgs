# tgs implementation and release plan

## Objective and success criteria

Build `tgs`, a production-grade, cross-platform command-line program that sends text notifications through Telegram. The core release succeeds when it can securely manage named profiles, accept exactly one message source, split and send plain text safely, expose stable machine-readable results and documented exit codes, and pass offline quality gates on Linux, macOS, and Windows.

## Locked product and technical decisions

- The executable is named `tgs`; `tgnotify` is the preferred alternative if renamed before publication.
- The repository uses Rust 2024 with an explicit MSRV and committed `Cargo.lock`.
- `main.rs` handles process I/O and exit status only. CLI parsing, configuration, input, splitting, transport, and output live in a small library.
- Runtime dependencies are limited to `ureq` (Rustls and JSON), `serde`, `serde_json`, and a small hidden-password-input crate. Dependencies must be pinned by Cargo and justified.
- The implementation is blocking and reuses one HTTP agent with enforced timeouts. It does not use async Rust, `curl`, media, plugins, or speculative extension points.
- Syntax is `tgs [send] [OPTIONS] [MESSAGE...]`; `send` is optional. A message comes from exactly one of positional words, `--message`, `--file`, or implicit piped stdin. An interactive invocation without a message prints help and exits.
- Send configuration includes profiles, destination override, parse mode, notification flags, topic, timeout, chunk cap, and silent/text/JSON output. `--verbose` aliases text output.
- Configuration is versioned JSON with a default profile and named token/chat profiles. Native platform paths, documented environment overrides, atomic same-directory writes, and restrictive Unix permissions are mandatory.
- Tokens are accepted only from hidden interactive input, `--token-stdin`, configuration, or `TGS_TOKEN`. They are never command-line send arguments, shown by `config show`, logged, or included in errors.
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
2. Profile: `--profile`, `TGS_PROFILE`, configured default.
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

- Current state: M0-M9 complete. The public-release pass added safe in-tool chat
  discovery, consistent validation, secret-safe debug behavior, hardened atomic
  persistence, a thinner binary, structural tests, and task-first documentation.
  Configuration and output schemas remain at version 1.
- Last verification: on 2026-08-27, Rust 1.85 compatibility, `cargo fmt --check`,
  locked all-target/all-feature check and strict Clippy, all 77 locked offline
  tests, warning-free rustdoc, the locked release build, dirty-worktree package
  verification/listing, and `cargo-audit` 0.22.2 against 1,226 advisories passed
  in Docker. Targeted splitting mutation testing caught all 47 viable mutants;
  one additional generated mutant was unviable. The package contains 20
  intentional files and no repository-only automation or planning files.
- Next action: review and commit M8-M9, push for Linux/macOS/Windows CI, then run
  one private live smoke test of bot creation, chat discovery, and delivery with
  disposable credentials before the public release. Confirm `tgs-cli` name
  availability immediately before any crates.io publication.
- Safety note: never place credentials in this file or repository history.
