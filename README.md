# tgs

`tgs` is a small cross-platform CLI for sending text notifications through the [Telegram Bot API](https://core.telegram.org/bots/api). It is designed for shell scripts, cron jobs, CI pipelines, and coding agents: success is quiet by default, failures are structured, and credentials never appear in command arguments or normal output.

Version 0.1 sends text only.

## Development and installation

Development uses the pinned Rust container in `.devcontainer` so the host does not need a Rust toolchain. Open the repository in a dev-container-aware editor, or run:

```sh
docker build -t tgs-dev:1.98 .devcontainer
docker run --rm -v "$PWD:/workspace" -w /workspace tgs-dev:1.98 cargo build --locked --release
```

The binary will be at `target/release/tgs` (`tgs.exe` on Windows). Published tags provide checksummed archives for the supported release targets. If building outside the container, Rust 1.85 or newer is required.

Releases are prepared from Conventional Commits by Release Please. Merging its release pull request updates the changelog and version, creates the tag and GitHub release, and triggers the platform archive workflow. The manifest is ready for crates.io packaging, but publication is neither automatic nor currently performed. Recheck the package name before the first publication.

CI and release actions are pinned to immutable commit SHAs. Dependabot proposes weekly GitHub Actions and Cargo updates, grouping compatible minor and patch dependency updates while leaving majors separate. A read-only weekly security workflow installs reviewed `cargo-audit` 0.22.2 exactly and fails when the committed lockfile contains a vulnerability advisory; it never edits dependencies or creates issues.

Runtime dependencies are deliberately narrow: `ureq` provides blocking Rustls HTTPS and JSON transport, `serde`/`serde_json` provide typed data, `rpassword` provides cross-platform hidden token entry, and `hostname` reads the local device name without a subprocess. `tgs` never invokes `curl` or another external executable.

## Telegram setup

1. Run `tgs bot create personal`. It opens the workflow by pointing you to [@BotFather](https://t.me/BotFather) and `/newbot`, then accepts the issued token through hidden input.
2. Start a conversation with the bot, or add it to the destination group/channel with permission to post.
3. Discover the destination ID by sending a message and inspecting `https://api.telegram.org/bot<TOKEN>/getUpdates` in a secure environment. Numeric private/group IDs and `@channel` usernames are supported. Do not paste the token into shared history, logs, or documentation.
4. Add the destination to the validated local profile, supplying the token again to replace it safely:

```sh
tgs bot add personal --chat-id 123456789
```

For non-interactive provisioning, place only the token on stdin:

```sh
printf '%s' "$BOT_TOKEN" | tgs bot add personal --chat-id 123456789 --token-stdin
```

Telegram remains responsible for bot creation. `tgs` does not use managed-bot deep links, host a manager bot, retrieve created tokens, or take custody of them.

## Sending

The `send` word is optional. Exactly one explicit message source may be used:

```sh
tgs deploy completed
tgs send --message "deploy completed"
tgs --file notification.txt
printf 'deploy completed\n' | tgs
```

With no explicit source, `tgs` reads piped stdin. With no source on an interactive terminal, it prints help instead of blocking.

Common automation patterns:

```sh
# Cron: identify the scheduler and use explicit config because its environment is minimal.
tgs --source cron --config /secure/path/config.json --profile ops "nightly backup completed"

# CI: use stable logical and runner names (environment variables work too).
build_command 2>&1 | tgs --source ci --device linux-runner --profile ci --output json

# Codex/coding agent: identify the integration and request machine-readable output.
tgs --source codex --profile agents --message "task finished" --output json
```

Telegram delivery flags include `--silent`, `--no-preview`, `--protect`, and `--topic ID`. The default timeout is 10 seconds. `--verbose` is an alias for `--output text`.

Every message is labeled automatically on its first line:

```text
[codex · workstation-01]
Task completed successfully
```

Set the logical source with `--source LABEL` or `TGS_SOURCE`; precedence is CLI, environment, then `tgs`. Set the device with `--device NAME` or `TGS_DEVICE`; precedence is CLI, environment, then the detected local hostname. If hostname detection fails or returns an invalid name, `unknown-device` is used so delivery can continue. Sources accept 1–64 Unicode characters and devices accept 1–255; leading/trailing whitespace and control characters are rejected. Generated values are escaped for plain text, HTML, and MarkdownV2.

Use `--no-label` when byte-for-byte preservation of the original message is required. It cannot be combined with explicit `--source` or `--device`; `TGS_SOURCE` and `TGS_DEVICE` are ignored in this mode. Labels are resolved for each invocation and are not stored in profiles. `tgs` does not discover or include IP addresses.

## Profiles and configuration

The friendly bot workflow validates every new token with Telegram `getMe` before writing configuration:

```text
tgs bot create PROFILE [--chat-id ID] [--config PATH]
tgs bot add PROFILE [--chat-id ID] [--token-stdin] [--config PATH]
tgs bot list [--config PATH]
tgs bot info [PROFILE] [--config PATH]
tgs bot use PROFILE [--config PATH]
tgs bot remove PROFILE [--config PATH]
```

`bot info` combines redacted local state with the live bot ID, display name, and optional `@username`; network failures are explicit. Re-adding a profile replaces its token only after successful validation and preserves its current chat ID unless `--chat-id` is supplied. `bot remove` removes only local configuration—the Telegram bot continues to exist.

The original schema-v1 configuration commands remain available:

```text
tgs config init [PROFILE]
tgs config set PROFILE [--chat-id ID] [--token-stdin]
tgs config list
tgs config show [PROFILE]
tgs config use PROFILE
tgs config remove PROFILE
tgs config path
```

`config show` reports only whether a token is set; it never reveals the token. Configuration is versioned JSON and is replaced through a temporary file in the same directory. On Unix, the directory and file are restricted to modes `0700` and `0600`.

Default locations:

- Linux: `$XDG_CONFIG_HOME/tgs/config.json`, or `$HOME/.config/tgs/config.json`
- macOS: `$HOME/Library/Application Support/tgs/config.json`
- Windows: `%APPDATA%\tgs\config.json`

Precedence, from strongest to weakest:

- Configuration path: `--config`, `TGS_CONFIG`, platform default
- Profile: `--profile`, `TGS_PROFILE`, configured default
- Token and destination: `TGS_TOKEN` / `TGS_CHAT_ID`, selected profile
- `--chat-id` overrides the resolved destination only
- Message source: `--source`, `TGS_SOURCE`, `tgs`
- Device: `--device`, `TGS_DEVICE`, detected hostname, `unknown-device` fallback

Environment credentials are useful for ephemeral CI jobs. Keep them out of debug traces and inherited environments where possible.

## Output and exit codes

Success is silent unless `--output text`, `--verbose`, or `--output json` is selected. JSON output has schema version 1:

```json
{"schema_version":1,"status":"ok","profile":"ops","destination":"@alerts","chunks_sent":1,"message_ids":[42]}
```

Failures go to stderr as JSON with `error` or `partial` status and any already-delivered message IDs. Tokens are excluded and token occurrences in Telegram descriptions are redacted.

| Code | Meaning |
|---:|---|
| 0 | Complete success |
| 2 | Usage, validation, or input error |
| 3 | Configuration or local filesystem error |
| 4 | Telegram, network, retry exhaustion, or partial delivery error |

## Long messages and retry safety

Plain text longer than Telegram's 4096-character limit is split on Unicode scalar boundaries, preferring newlines. Every chunk receives the label, and its header plus newline counts toward that chunk's 4096-character limit. `--max-chunks` defaults to 10 and is checked before any request. HTML and MarkdownV2 remain unsplittable and are rejected when the escaped header plus message exceeds the limit, because arbitrary splitting can break formatting. The public library `prepare_message` behavior remains unchanged for callers that prepare unlabeled content directly.

Chunks are delivered sequentially. If a later chunk fails, stderr reports `partial` and the message IDs already delivered. Only explicit HTTP 429 replies are retried—at most twice, honoring Telegram's `retry_after` up to 60 seconds. Timeouts, transport errors, 5xx replies, and ambiguous responses are not automatically retried, avoiding accidental duplicates.

## Security

- Never pass a bot token as an ordinary command-line argument; `tgs` has no token send flag.
- Prefer hidden entry, `--token-stdin`, or a short-lived `TGS_TOKEN` environment value.
- Protect configuration backups and do not commit `config.json`.
- JSON/text output contains destinations and Telegram message IDs, but never tokens.
- The default label discloses the local hostname to the Telegram chat. Use a generic
  `--device`/`TGS_DEVICE` value or `--no-label` where hostnames are sensitive.
- `tgs` never performs local or public IP discovery for labels.
- Treat a reported partial delivery as delivered; do not blindly rerun the full message.

## Local quality and packaging gates

Run the complete release-readiness gate in the pinned development environment:

```sh
cargo fmt --check
cargo check --locked --all-targets --all-features
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --all-features
cargo build --locked --release
cargo package --locked
cargo audit
```

`cargo package --locked` validates the publishable archive. It includes the manifest, lockfile, license, README, changelog, source, and tests while excluding repository automation, devcontainer, agent, planning, and Release Please files.

## License

MIT
