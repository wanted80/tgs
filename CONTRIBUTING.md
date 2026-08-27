# Contributing

Use the pinned development container in `.devcontainer`, or build the same image with Docker. Do not use real Telegram credentials in tests, fixtures, issues, or commits.

Before submitting a change, update tests and run:

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

CI runs the stable quality suite on Linux, macOS, and Windows and a separate Rust 1.85 MSRV job. Linux stable also treats rustdoc warnings as errors and validates the crate package. The security workflow audits the committed lockfile weekly and on manifest or lockfile changes; resolve advisories through a reviewed dependency update rather than an automated workflow mutation.

Before the first crates.io release, inspect `cargo package --list`, recheck package-name availability, and publish manually only after explicit release approval. Publication is intentionally not automated.

Keep changes focused, add a changelog entry for user-visible behavior, and update `PLAN.md` if scope or architecture changes.

Use [Conventional Commits](https://www.conventionalcommits.org/), such as `feat:`, `fix:`, `docs:`, `test:`, and `chore:`. Release Please uses these commit types to prepare release pull requests, changelogs, and version bumps.
