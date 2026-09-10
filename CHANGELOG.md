# Changelog

All notable changes are documented here. This project follows Semantic Versioning.

## [Unreleased]

### Added

- `tgs setup [PROFILE]`: guided hidden-token input, automatic private/group chat
  pairing, optional terminal QR codes, destination confirmation, atomic saving,
  and an optional test notification.
- `tgs status [PROFILE] [--output text|json]`: read-only credential, destination,
  and webhook diagnostics using the same overrides as sends.
- Complete offline command workflow coverage with injectable terminal I/O.
- Interactive private-chat/group/channel selection and setting origins in text
  and JSON status reports, without displaying credential values.

### Changed

- Select the only profile automatically; environment-only credentials no longer
  require creating or naming a profile.
- Display readable errors on terminals and retain JSON for redirected stderr or
  explicit JSON output. Keep delivery IDs when success output cannot be written.
- Limit timeouts to 1-300 seconds and bound message, token, and configuration
  input. Bound each splitting attempt by the chunk cap.
- Keep top-level help focused on setup, sending, and status; provide dedicated
  help for every existing command while preserving commands and options.

### Fixed

- Pin setup's suggested send command to the confirmed profile and destination
  when environment overrides remain active, and explain those overrides.
- Use `TGS_PROFILE` consistently in `bot info`, `bot chats`, and `config show`;
  explicitly identify these queries as saved-credential views.
- Reject unsupported message controls before delivery and prevent
  Telegram-normalized invisible text from forming empty chunks.
- Split feasible long-whitespace messages safely and relax newline preferences
  when they would otherwise exhaust the chunk budget unnecessarily.
- Pass Rust 1.85 Clippy checks for pairing-code encoding and sender validation,
  preserving the existing pairing behavior and strict CI settings.
- Prevent malformed configuration values from appearing in errors.
- Return stable errors for non-Unicode arguments, overflowing timeout values,
  and failed stdout writes/flushes instead of panicking.
- Shut down local test servers even when expected requests never arrive.
- Disable HTTP redirects for credential-bearing Telegram requests.
- Reject invalid encoding in selected environment values instead of silently
  using saved credentials or another destination; preserve explicit overrides.
- Rebalance newline/whitespace chunk boundaries and reject blank chunks before
  delivery, including label-only notifications.
- Redact active tokens from successful bot identity and chat-discovery metadata.
- Check configuration snapshots and atomically replace files under an OS-backed
  lock, preventing concurrent CLI updates from silently overwriting each other.
- Diagnose existing update filters that prevent setup pairing, without changing
  those filters or asking users to retry an impossible pairing.
- Accept `--` before option-like profile names across setup, status, bot, and
  configuration commands, and reject overflowing numeric chat IDs locally.
- Bound shutdown of timeout-test fixtures, including incomplete requests; replace
  fixture sleeps with explicit shutdown and distinguish actual request timeouts
  from other network failures.

## [0.3.0](https://github.com/wanted80/tgs/compare/v0.2.0...v0.3.0) (2026-08-27)


### Features

* enhance public release readiness with safety and validation improvements ([#6](https://github.com/wanted80/tgs/issues/6)) ([340c5fd](https://github.com/wanted80/tgs/commit/340c5fd931533183a759389b0784d62b955a4126))

### Added

- Automatic `[source · device]` headers with safe formatting-mode escaping,
  deterministic overrides, hostname fallback, and a `--no-label` opt-out.
- Read-only `tgs bot chats` destination discovery without token-bearing URLs.
- Architecture, malformed-configuration, external-data, atomic-replacement, and
  secret-safe debug regression tests, plus mutation-tested split boundaries.

### Changed

- Reworked the README around installation and a short first-run workflow.
- Moved configuration operations behind testable library functions and made CLI
  help available at nested command positions.
- Strengthened token, destination, configuration invariant, and external display
  validation before writes, requests, or terminal output.

### Fixed

- Derive help text version from the package instead of reporting `0.1.0` forever.
- Preserve atomic configuration replacement on Windows instead of deleting the
  destination before rename, and create collision-safe temporary files.
- Prevent environment and configuration debug formatting from exposing secrets,
  and ignore non-Unicode process environment values without panicking.
- Removed the debug-build runtime API endpoint override from production sources.

## [0.2.0](https://github.com/wanted80/tgs/compare/v0.1.0...v0.2.0) (2026-08-27)


### Features

* enhance bot management and harden CI for cross-platform support ([#4](https://github.com/wanted80/tgs/issues/4)) ([09f2e84](https://github.com/wanted80/tgs/commit/09f2e84a2137a2325578132a046ad388928f5030))

## 0.1.0 (2026-08-26)


### Features

* implement secure Telegram notification CLI ([c240b7e](https://github.com/wanted80/tgs/commit/c240b7e5766f20d61a66e3d321da4e47691f0e9d))


### Bug Fixes

* satisfy platform clippy checks ([7b5d7c9](https://github.com/wanted80/tgs/commit/7b5d7c9516ccd454283e16e6ea108e8a5adc06d1))
