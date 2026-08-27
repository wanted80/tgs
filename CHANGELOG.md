# Changelog

All notable changes are documented here. This project follows Semantic Versioning.

## [Unreleased]

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
