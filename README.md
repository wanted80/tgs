# tgs

`tgs` sends text notifications through the [Telegram Bot API](https://core.telegram.org/bots/api) from a terminal, script, scheduled job, CI runner, or coding agent.

It is intentionally small: one binary, quiet success by default, actionable JSON errors, named bot profiles, and no external HTTP command such as `curl`. Tokens are entered through a hidden prompt, stdin, configuration, or the environment—never as a normal command-line argument.

## Install

Download the archive for your platform from [GitHub Releases](https://github.com/wanted80/tgs/releases), verify its adjacent SHA-256 checksum, and place `tgs` (`tgs.exe` on Windows) somewhere on `PATH`.

Release archives are built for:

- Linux x86-64 and ARM64
- macOS Intel and Apple Silicon
- Windows x86-64

After the crate is published to crates.io, Rust users can instead run:

```sh
cargo install tgs-cli --locked
```

Rust 1.85 or newer is required when building from source.

## Set up your first notification

1. Create a local profile and a Telegram bot:

   ```sh
   tgs bot create personal
   ```

   `tgs` guides you to [@BotFather](https://t.me/BotFather), accepts the issued token through a hidden prompt, validates it with Telegram, and stores it locally.

2. Open the new bot in Telegram and send it a message such as `/start`. For a group, add the bot and send a message in that group.

3. Find the destination without putting the token in a URL or shell history:

   ```sh
   tgs bot chats personal
   ```

   Choose the displayed numeric ID, then save it:

   ```sh
   tgs config set personal --chat-id 123456789
   ```

   Supplying only `--chat-id` keeps the existing token and does not prompt for it again. Recent updates are retained by Telegram for at most 24 hours; `bot chats` does not confirm or delete them. It cannot be used while the bot has an active webhook.

4. Send a test notification:

   ```sh
   tgs --verbose "Hello from tgs"
   ```

   `--verbose` prints a delivery summary. Without it, successful sends are quiet, which is convenient in scripts.

For a public channel, make the bot an administrator that can post and use the channel username directly:

```sh
tgs bot create alerts --chat-id @my_channel
```

## Send messages

The `send` word is optional. Choose exactly one message source:

```sh
tgs deploy completed
tgs send --message "deploy completed"
tgs --file notification.txt
printf 'deploy completed\n' | tgs
```

With no explicit source, piped stdin is read. An interactive invocation with no message prints help instead of waiting forever.

Useful delivery options:

```text
--profile NAME          Select a profile
--chat-id ID            Override its destination for this send
--silent                Disable the Telegram notification sound
--no-preview            Disable link previews
--protect               Restrict forwarding and saving
--topic ID              Send to a forum topic
--parse-mode MODE       plain, html, or markdown-v2
--timeout SECONDS       Request timeout (default: 10)
--max-chunks N          Plain-text safety cap (default: 10)
--output MODE           silent, text, or json
```

Run `tgs --help` for the complete command reference. Help is also accepted after a command, for example `tgs bot create --help`.

## Message labels

Every outgoing message starts with its source and device:

```text
[codex · workstation-01]
Task completed successfully
```

The source defaults to `tgs`; the device defaults to the local hostname. Override them per invocation:

```sh
tgs --source cron --device backup-runner "nightly backup completed"
```

`TGS_SOURCE` and `TGS_DEVICE` provide environment defaults. CLI values take precedence. Labels are escaped safely in plain text, HTML, and MarkdownV2.

Use `--no-label` when the original content must be preserved exactly. This also ignores label environment variables. The automatic label never discovers or includes an IP address, but the hostname itself may be sensitive; use a generic device name or disable the label when needed.

## Profiles and configuration

Friendly bot commands validate tokens through Telegram:

```text
tgs bot create PROFILE [--chat-id ID]
tgs bot add PROFILE [--chat-id ID] [--token-stdin]
tgs bot list
tgs bot info [PROFILE]
tgs bot chats [PROFILE]
tgs bot use PROFILE
tgs bot remove PROFILE
```

All bot commands accept `--config PATH`. `bot add` adds or replaces a token. An omitted `--chat-id` preserves the current destination. `bot info` combines redacted local state with the live bot identity. `bot remove` deletes only the local profile; delete or revoke the Telegram bot through BotFather.

Local configuration commands are available for offline profile editing:

```text
tgs config init [PROFILE]
tgs config set PROFILE [--chat-id ID] [--token-stdin]
tgs config list
tgs config show [PROFILE]
tgs config use PROFILE
tgs config remove PROFILE
tgs config path
```

All configuration commands accept `--config PATH`. `config show` reports only whether a token is set. Configuration is versioned JSON and is replaced atomically in the same directory. On Unix, files created by `tgs` use mode `0600`; a newly created configuration directory uses `0700`.

Default locations:

- Linux: `$XDG_CONFIG_HOME/tgs/config.json`, or `$HOME/.config/tgs/config.json`
- macOS: `$HOME/Library/Application Support/tgs/config.json`
- Windows: `%APPDATA%\tgs\config.json`

Resolution order, strongest first:

| Value | Precedence |
|---|---|
| Configuration path | `--config`, `TGS_CONFIG`, platform default |
| Profile | `--profile`, `TGS_PROFILE`, configured default |
| Token | `TGS_TOKEN`, selected profile |
| Destination | `--chat-id`, `TGS_CHAT_ID`, selected profile |
| Source label | `--source`, `TGS_SOURCE`, `tgs` |
| Device label | `--device`, `TGS_DEVICE`, hostname, `unknown-device` |

Environment credentials are useful for ephemeral CI jobs. Keep them short-lived and out of debug traces and broadly inherited environments.

## Automation examples

```sh
# Cron: use an explicit configuration path because its environment is minimal.
tgs --source cron --config /secure/path/config.json --profile ops "backup completed"

# CI: pipe command output and request a machine-readable success report.
build_command 2>&1 | tgs --source ci --device linux-runner --profile ci --output json

# Coding agent: identify the integration and destination profile.
tgs --source codex --profile agents --message "task finished" --output json
```

## Output and exit codes

Success is silent unless `--output text`, `--verbose`, or `--output json` is selected. JSON success output uses schema version 1:

```json
{"schema_version":1,"status":"ok","profile":"ops","destination":"@alerts","chunks_sent":1,"message_ids":[42]}
```

Failures are written to stderr as schema-versioned JSON. A failure after one or more chunks were delivered has `"status":"partial"` and includes their message IDs.

| Code | Meaning |
|---:|---|
| 0 | Complete success |
| 2 | Invalid arguments, input, or locally validated values |
| 3 | Configuration or local filesystem failure |
| 4 | Telegram, network, retry exhaustion, or partial delivery failure |

## Long messages and retry safety

Plain text over Telegram's 4096-character limit is split on Unicode scalar boundaries, preferring newlines. The label is repeated on every chunk and counts toward the limit. `--max-chunks` defaults to 10 and is checked before any request.

Oversized HTML and MarkdownV2 are rejected because arbitrary splitting could break formatting. Use plain text for automatic splitting.

Chunks are delivered sequentially. Only an explicit HTTP 429 response is retried, at most twice, honoring Telegram's `retry_after` up to 60 seconds. Network failures, timeouts, 5xx replies, and ambiguous responses are not retried automatically, avoiding accidental duplicates. Treat a partial-delivery report as delivered; do not blindly rerun the whole message.

## Security notes

- Never pass a bot token as an ordinary argument. `tgs` deliberately has no token option for sends.
- Prefer hidden input, `--token-stdin`, or a short-lived `TGS_TOKEN` value.
- Protect configuration backups and never commit `config.json`.
- Human, JSON, error, and debug output never include token values.
- Use `tgs bot chats` instead of constructing a token-bearing Telegram URL.
- Destinations and Telegram message IDs are included in requested reports; treat them according to your own privacy needs.
- The default label discloses the hostname to the destination chat unless overridden or disabled.

## Development

See [CONTRIBUTING.md](CONTRIBUTING.md) for the pinned development environment, test layers, release-quality gates, and contribution conventions. The runtime dependencies remain narrow and purposeful: `ureq` for blocking Rustls HTTPS and JSON transport, `serde`/`serde_json` for typed data, `rpassword` for hidden token input, and `hostname` for subprocess-free device naming.

## License

[MIT](LICENSE)
