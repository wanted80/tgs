# tgs v0.1 implementation plan

## Objective and success criteria

Build `tgs`, a production-grade, cross-platform command-line program that sends text notifications through Telegram. Version 0.1 succeeds when it can securely manage named profiles, accept exactly one message source, split and send plain text safely, expose stable machine-readable results and documented exit codes, and pass offline quality gates on Linux, macOS, and Windows.

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
- Version 0.1 sends text only. Command execution, attachments, keyboards, rich messages, and OS credential stores are deferred.
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
- [ ] M1: Add engineering guidelines and repository/legal scaffolding.
- [ ] M2: Implement CLI, configuration, message validation/splitting, and output models.
- [ ] M3: Implement Telegram transport, retry policy, and partial-delivery reporting.
- [ ] M4: Add unit, integration, and end-to-end tests with no external network dependency.
- [ ] M5: Add documentation, CI, release packaging, and complete all quality gates.

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

## Resume here

- Current state: M0 complete; repository initialized on `main` and the living plan created as the first artifact.
- Last verification: Git repository initialized with repository-local author identity.
- Next action: Commit this plan, then create and commit `AGENTS.md` before adding Rust code.
- Safety note: never place credentials in this file or repository history.
