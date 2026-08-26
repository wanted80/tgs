# Contributing

Use the pinned development container in `.devcontainer`, or build the same image with Docker. Do not use real Telegram credentials in tests, fixtures, issues, or commits.

Before submitting a change, update tests and run:

```sh
cargo fmt --check
cargo check --locked
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
cargo build --locked --release
```

Keep changes focused, add a changelog entry for user-visible behavior, and update `PLAN.md` if scope or architecture changes.
