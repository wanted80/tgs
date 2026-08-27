# Changelog

All notable changes are documented here. This project follows Semantic Versioning.

## [0.2.0](https://github.com/wanted80/tgs/compare/v0.1.0...v0.2.0) (2026-08-27)


### Features

* enhance bot management and harden CI for cross-platform support ([#4](https://github.com/wanted80/tgs/issues/4)) ([09f2e84](https://github.com/wanted80/tgs/commit/09f2e84a2137a2325578132a046ad388928f5030))

## 0.1.0 (2026-08-26)


### Features

* implement secure Telegram notification CLI ([c240b7e](https://github.com/wanted80/tgs/commit/c240b7e5766f20d61a66e3d321da4e47691f0e9d))


### Bug Fixes

* satisfy platform clippy checks ([7b5d7c9](https://github.com/wanted80/tgs/commit/7b5d7c9516ccd454283e16e6ea108e8a5adc06d1))

## [Unreleased]

### Added

- Initial `tgs` command-line interface, profile configuration, and Telegram text delivery.
- Friendly `tgs bot` creation guidance and validated add/list/info/use/remove profile management.
- Live, injectable Telegram `getMe` identity validation before bot-profile writes.
- Crate package validation, centralized lint policy, Cargo Dependabot updates, and weekly RustSec auditing.
