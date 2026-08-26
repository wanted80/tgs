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

Releases are prepared from Conventional Commits by Release Please. Merging its release pull request updates the changelog and version, creates the tag and GitHub release, and triggers the platform archive workflow. CI and release actions are pinned to immutable commit SHAs; Dependabot proposes reviewed action updates.

Runtime dependencies are deliberately narrow: `ureq` provides blocking Rustls HTTPS and JSON transport, `serde`/`serde_json` provide typed data, and `rpassword` provides cross-platform hidden token entry. `tgs` never invokes `curl` or another external executable.

## Telegram setup

1. In Telegram, open [@BotFather](https://t.me/BotFather), run `/newbot`, and retain the bot token securely.
2. Start a conversation with the bot, or add it to the destination group/channel with permission to post.
3. Discover the destination ID by sending a message and inspecting `https://api.telegram.org/bot<TOKEN>/getUpdates` in a secure environment. Numeric private/group IDs and `@channel` usernames are supported. Do not paste the token into shared history, logs, or documentation.
4. Initialize and populate a profile:

```sh
tgs config init personal
tgs config set personal --chat-id 123456789
# The second command prompts for the token with input hidden.
```

For non-interactive provisioning, place only the token on stdin:

```sh
printf '%s' "$BOT_TOKEN" | tgs config set personal --token-stdin
```

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
# Cron: use explicit config because cron environments are minimal.
tgs --config /secure/path/config.json --profile ops "nightly backup completed"

# Pipeline: notify with command output without executing it inside tgs.
build_command 2>&1 | tgs --profile ci --output json

# Coding agent: request stable metadata for downstream parsing.
tgs --profile agents --message "task finished" --output json
```

Telegram delivery flags include `--silent`, `--no-preview`, `--protect`, and `--topic ID`. The default timeout is 10 seconds. `--verbose` is an alias for `--output text`.

## Profiles and configuration

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

Plain text longer than Telegram's 4096-character limit is split on Unicode scalar boundaries, preferring newlines. `--max-chunks` defaults to 10 and is checked before any request. Oversized HTML and MarkdownV2 are rejected because arbitrary splitting can break formatting.

Chunks are delivered sequentially. If a later chunk fails, stderr reports `partial` and the message IDs already delivered. Only explicit HTTP 429 replies are retried—at most twice, honoring Telegram's `retry_after` up to 60 seconds. Timeouts, transport errors, 5xx replies, and ambiguous responses are not automatically retried, avoiding accidental duplicates.

## Security

- Never pass a bot token as an ordinary command-line argument; `tgs` has no token send flag.
- Prefer hidden entry, `--token-stdin`, or a short-lived `TGS_TOKEN` environment value.
- Protect configuration backups and do not commit `config.json`.
- JSON/text output contains destinations and Telegram message IDs, but never tokens.
- Treat a reported partial delivery as delivered; do not blindly rerun the full message.

## License

MIT
