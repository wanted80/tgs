# Contributing to tgs

Keep changes small, cross-platform, and easy to review. Never use real Telegram credentials in tests, fixtures, examples, issues, or commits.

## Development environment

Use the pinned container in `.devcontainer`, either through a compatible editor or directly:

```sh
docker build -t tgs-dev:1.98 .devcontainer
docker run --rm -v "$PWD:/workspace" -w /workspace tgs-dev:1.98 cargo test --locked --all-targets --all-features
```

The host does not need Rust. If you work outside the container, use Rust 1.85 or newer; CI checks both the current stable toolchain and the declared MSRV.

## Where tests belong

- Module unit tests cover parsing, validation, splitting, redaction, configuration invariants, and exact boundary behavior.
- `tests/transport.rs` uses a local HTTP server for Telegram success, malformed data, retry, timeout, redaction, and partial-delivery behavior. It must never contact the public network.
- `tests/cli_e2e.rs` runs the compiled binary for stdin/stdout/stderr, exit-code, and local configuration workflows.
- `tests/architecture.rs` protects the thin-binary/library boundary and prevents runtime test endpoints from entering production sources.

Prefer exact outcomes over `is_err()` alone: assert the error category, message, side effects, request count, and redaction behavior that matter. Every behavior change needs focused tests; every bug fix needs a regression test.

For a deeper periodic check, [cargo-mutants](https://mutants.rs/) can identify assertions that execute code without constraining its behavior:

```sh
cargo install --locked cargo-mutants
cargo mutants --all-features
```

Mutation testing is intentionally not a per-commit CI gate because a full run is expensive. Use it before significant releases or after changing validation, splitting, persistence, or retry logic; investigate every surviving mutant rather than chasing a percentage target.

## Required quality gates

Run all of these before considering a change complete:

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

Install the same reviewed audit version used by CI if needed:

```sh
cargo install cargo-audit --version 0.22.2 --locked
```

`cargo package --locked` validates the archive that would be published. Before the first crates.io release, inspect `cargo package --list`, confirm that the package name is available and correct, and publish manually only after explicit approval. Crate publication is intentionally not automated.

## Pull requests and releases

- Keep `PLAN.md` current when scope or architecture changes and record route changes in its decision log.
- Add user-visible changes under `Unreleased` in `CHANGELOG.md`.
- Use [Conventional Commits](https://www.conventionalcommits.org/), such as `feat:`, `fix:`, `docs:`, `test:`, and `chore:`.
- Do not broaden dependencies without a concrete purpose and conservative features.
- Preserve stable exit-code categories and schema-versioned machine output.

CI runs the full stable suite on Linux, macOS, and Windows plus a Rust 1.85 MSRV job. Linux stable also denies rustdoc warnings and validates the crate package. A separate minimum-permission workflow audits the committed lockfile weekly and whenever dependencies change.

Release Please prepares version, changelog, tag, and GitHub release changes from Conventional Commits. A tag-triggered workflow builds checksummed platform archives; crates.io publication remains a separate manual decision.
