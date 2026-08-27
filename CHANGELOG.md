# Changelog

All notable changes are documented here. This project follows Semantic Versioning.

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
