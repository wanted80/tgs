# tgs

`tgs` sends text notifications through the [Telegram Bot API](https://core.telegram.org/bots/api) from a terminal, script, scheduled job, CI runner, or coding agent.

It is intentionally small: one binary, guided setup, quiet success by default, readable terminal errors and JSON errors for automation, and no external HTTP command such as `curl`. Tokens are entered through a hidden prompt, stdin, configuration, or the environment—never as a normal command-line argument.

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

1. Run the setup wizard in a terminal:

   ```sh
   tgs setup
   ```

   If you need a bot, the wizard guides you to [@BotFather](https://t.me/BotFather). Paste the issued token through the hidden prompt, or reuse an existing bot. Your first profile is named `default` automatically.

2. Open the pairing link displayed by the wizard and press **Start** in Telegram. Return to the terminal and press Enter. `tgs` detects the destination; confirm its name to save the completed configuration. You do not need to copy a chat ID.

3. Accept the optional test notification, or send your own:

   ```sh
   tgs "Hello from tgs"
   ```

   Success is quiet by default. Add `--verbose` for a delivery summary.

Useful setup variants:

```sh
tgs setup --qr                       # Scan the pairing link on your phone
tgs setup team --group               # Choose a group in Telegram
tgs setup alerts --chat-id @my_channel
tgs setup --config /secure/path/config.json
```

For a channel, first make the bot an administrator with posting permission. `--chat-id` also supports known numeric destinations and bots already used by another application. `--group` and `--qr` request fresh pairing and ignore `TGS_CHAT_ID` and a saved destination.

The wizard saves nothing until you confirm the destination. A failed optional test leaves the completed configuration in place. It reuses saved credentials on request and asks before persisting `TGS_TOKEN`. Existing configuration commands remain available for noninteractive setup.

Pairing links contain a random code accepted only by the current setup session, never your bot token. Pairing does not acknowledge or discard Telegram updates, change update filters, or remove webhooks. Setup checks for an active webhook or an existing `allowed_updates` filter that disables message updates and explains when to use `--chat-id` or a dedicated notification bot. The same alternatives apply if another application consumes the pairing message or the pending queue is full. If a message has not arrived, the wizard lets you retry or cancel.

Check the current connection without sending a notification:

```sh
tgs status
tgs status team --output json
```

Status validates the resolved token, looks up the destination, and reports whether a webhook is active. It does not prove permission for every send option; use the wizard's test notification to check delivery. This is a standalone Bot API client: bot creation still uses BotFather, with no hosted manager or notification relay.

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
--timeout SECONDS       Request timeout, 1-300 seconds (default: 10)
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

For manual discovery, send the bot a message and run `tgs bot chats`, then use `tgs config set PROFILE --chat-id ID`. Telegram keeps updates for at most 24 hours. Chat discovery does not confirm or delete them and cannot run while a webhook is active.

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

CLI changes also compare the original configuration under an OS-backed lock before saving. If another command has changed it or a save is in progress, `tgs` fails without overwriting that change; retry the command. An empty `.tgs-config.lock` file remains in the configuration directory and contains no credentials. Its name is reserved: do not use it as the configuration path or delete it while commands are running. The OS releases the lock when the process exits, including after a crash. Other editors do not honor this lock; avoid editing the file manually during a save.

For a profile name beginning with a hyphen, put options first and use `--` before the name:

```sh
tgs setup -- -ops
tgs config show -- -ops
tgs bot remove -- -ops
```

Default locations:

- Linux: `$XDG_CONFIG_HOME/tgs/config.json`, or `$HOME/.config/tgs/config.json`
- macOS: `$HOME/Library/Application Support/tgs/config.json`
- Windows: `%APPDATA%\tgs\config.json`

Resolution order, strongest first:

| Value | Precedence |
|---|---|
| Configuration path | `--config`, `TGS_CONFIG`, platform default |
| Profile | `--profile`, `TGS_PROFILE`, configured default, only profile |
| Token | `TGS_TOKEN`, selected profile |
| Destination | `--chat-id`, `TGS_CHAT_ID`, selected profile |
| Source label | `--source`, `TGS_SOURCE`, `tgs` |
| Device label | `--device`, `TGS_DEVICE`, hostname, `unknown-device` |

Environment credentials are useful for ephemeral CI jobs. Keep them short-lived and out of debug traces and broadly inherited environments.

An environment value with invalid text encoding produces an error when selected; it is never silently ignored in favor of saved credentials or a different destination. Explicit CLI overrides still win, and `--no-label` still ignores unused label settings. Numeric chat IDs must fit a nonzero signed 64-bit integer.

One configured bot needs no explicit profile selection. With an empty configuration, `TGS_TOKEN` and `TGS_CHAT_ID` are sufficient to send or check status; the report uses profile name `default` and no configuration is written. Multiple profiles without a default require an explicit selection. Setup and status also accept a positional profile name.

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

Failures are readable text when stderr is a terminal. Redirected stderr, or an explicit `--output json`, produces schema-versioned JSON. A failure after one or more chunks were delivered has `"status":"partial"` and includes their message IDs. If delivery succeeds but writing or flushing the success report fails, exit code 3 retains those IDs and tells you not to resend.

| Code | Meaning |
|---:|---|
| 0 | Complete success |
| 2 | Invalid arguments, input, or locally validated values |
| 3 | Configuration, local filesystem, or output failure |
| 4 | Telegram, network, retry exhaustion, or partial delivery failure |

## Long messages and retry safety

Plain text over Telegram's 4096-character limit is split on Unicode scalar boundaries, preferring newlines. The label is repeated on every chunk and counts toward the limit. `--max-chunks` defaults to 10 and is checked before any request.

Splitting keeps non-whitespace text in every chunk, moving a boundary when needed so a trailing newline cannot create an empty notification. Whitespace-only messages and excessive whitespace that would require a blank chunk are rejected before sending anything; `tgs` does not silently discard that input. Labels do not turn blank chunks into valid message bodies. Telegram itself may normalize whitespace when displaying a message.

Message reading is bounded by `4096 × 4 × --max-chunks` bytes (four bytes per possible Unicode scalar), with an absolute ceiling of 16 MiB. The actual character and chunk limits are checked separately. Splitting stops as soon as the cap is exceeded. Configuration files are limited to 1 MiB and token input to 4 KiB. Non-Unicode command arguments are rejected with a usage error; UTF-8 message content is supported.

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

See [CONTRIBUTING.md](CONTRIBUTING.md) for the pinned development environment, test layers, release-quality gates, and contribution conventions. Dependencies have focused purposes: `ureq` for blocking Rustls HTTPS, `serde`/`serde_json` for typed data, `rpassword` for hidden input, `hostname` for device naming, `getrandom` for secure pairing codes, `qrcode` for optional terminal QR display (without image dependencies), and `fs2` for cross-platform OS file locks on Rust 1.85 (standard-library file locks require Rust 1.89).

## License

[MIT](LICENSE)
