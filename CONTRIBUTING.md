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
- `tests/transport.rs` uses a local HTTP server for Telegram success, malformed data, retry, timeout, redaction, and partial-delivery behavior. It must never contact the public network. Timeout fixtures withhold a response until explicit shutdown, with a bounded fallback; do not add sleeps or unbounded socket waits. Assert that a timeout is actually reported, not merely any network error.
- `tests/cli_e2e.rs` runs the compiled binary for stdin/stdout/stderr, exit-code, native environment decoding, cross-process configuration locking, and local configuration workflows.
- `tests/workflows.rs` exercises the command workflows with injected terminal I/O and local HTTP, including successful sends in every output mode, setup pairing/cancellation, and status. `tests/support` supplies a local server with explicit shutdown so missing requests do not leave the test process blocked in `accept()`.
- Setup regressions should execute the suggested follow-up command, not only
  inspect the saved file. Check environment overrides, destination choices,
  saved-versus-effective credential views, and status setting origins.
- Splitting tests include an independent exhaustive partition oracle: verify
  that feasible inputs are accepted, not only that successful output is valid.
  Cover Telegram-normalized empty characters, control rejection, byte-for-byte
  preservation, and fallback from newline preferences under tight chunk caps.
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

Repeat formatting, check, strict Clippy, tests, and the release build with Rust
1.85 as well as the pinned development toolchain. Install that toolchain's
`rustfmt` and `clippy` components inside Docker. A passing MSRV check and test
run alone is not the full CI gate: Clippy diagnostics can differ across versions.

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

Release Please prepares version and changelog pull requests from Conventional
Commits. It must use `config-file` and `manifest-file` inputs: adding a
`release-type` workflow input bypasses the JSON configuration, including the
draft setting. The configuration keeps unprefixed `vX.Y.Z` tags and explicitly
creates the tag for each draft release.

After creating the draft, Release Please calls the reusable binary workflow with
its tag output. A tag push alone does not start binary publication: Release
Please creates the tag before the draft exists. The workflow checks that the
release is a mutable draft, builds the five targets from that tag, and publishes
only after every archive and SHA-256 upload succeeds. Each GitHub CLI operation
selects the repository explicitly. Runs for the same tag are serialized.

If a build or upload fails, the release stays a draft. Rerun failed jobs, or
dispatch **Release binaries** from `main` with the existing draft tag as the
`tag` input. Successful uploads can be replaced while the release remains a
draft. Once published, rerunning an upload is rejected at preflight; use a new
version for an already sealed release. `v0.4.0` and `v0.4.1` were sealed without
binaries and cannot be repaired by another upload. Crates.io publication
remains a separate manual decision.

CI separately checks that Release Please uses manifest mode and that draft/tag
settings remain enabled. Workflow edits should also pass `actionlint`
(validated with 1.7.12).
