# Engineering guidelines for tgs

These instructions apply to the entire repository.

## Design

- Prefer the smallest readable design that solves the current requirement. Avoid clever abstractions, premature optimization, and speculative extension points.
- Write stable, idiomatic, production-grade Rust that behaves consistently on Linux, macOS, and Windows.
- Keep `main.rs` limited to process I/O and exit handling. Put testable behavior in the library.
- Prefer the standard library. Every dependency must have a concrete purpose, conservative features, and a non-wildcard version constraint.
- Do not use `unsafe` code, async Rust, subprocesses for HTTP, or undocumented platform assumptions.

## Correctness and security

- Model failures with explicit, actionable error types and stable exit-code categories.
- Never use `unwrap`, `expect`, or deliberate panics in production paths. Tests may use them when the failure message remains clear.
- Never log, format, serialize, or return tokens or token-bearing Telegram URLs. Treat environment values and configuration contents as secret-bearing data.
- Validate all user-controlled values before side effects. Do not make partial configuration writes or network requests after known validation failures.
- Use atomic configuration replacement and restrictive permissions where the operating system supports them.
- Keep functions focused, make ownership obvious, and document public behavior and non-obvious invariants.

## Testing

- Add regression tests with every bug fix and focused tests with every behavior change.
- Tests must be deterministic and independent of Telegram, credentials, wall-clock delays, and the public network.
- Use temporary, isolated state for filesystem tests and local servers for transport tests.
- Exercise Unicode, boundary values, malformed external data, redaction, and partial failures—not only happy paths.

## Quality gates

Before considering a change complete, run:

```text
cargo fmt --check
cargo check --locked
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
cargo build --locked --release
```

Keep `PLAN.md` current before changing scope or architecture and after each milestone. Record route changes in its decision log and update its “Resume here” section. Never put credentials in documentation, fixtures, snapshots, commits, or command examples.
