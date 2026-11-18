#![allow(clippy::expect_used, clippy::panic)]
//! Lightweight structural tests for release-critical architecture boundaries.

use std::fs;
use std::path::PathBuf;

fn source(path: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);
    fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read architecture input `{}`: {error}",
            path.display()
        )
    })
}

#[test]
fn binary_entrypoint_contains_io_but_no_storage_or_transport_implementation() {
    let main = source("src/main.rs");
    for required in ["fn main()", "read_message", "read_token", "ExitCode"] {
        assert!(main.contains(required), "main.rs must retain `{required}`");
    }
    for forbidden in [
        "ureq::",
        "TelegramClient",
        "Config::load",
        "Config::save",
        "Profile {",
        "serde::Deserialize",
        "serde::Serialize",
    ] {
        assert!(
            !main.contains(forbidden),
            "main.rs must delegate `{forbidden}` behavior to the library"
        );
    }
}

#[test]
fn production_sources_contain_no_runtime_test_endpoint_override() {
    for module in [
        "src/app.rs",
        "src/input.rs",
        "src/setup.rs",
        "src/bot.rs",
        "src/cli.rs",
        "src/config.rs",
        "src/error.rs",
        "src/help.rs",
        "src/lib.rs",
        "src/main.rs",
        "src/output.rs",
        "src/split.rs",
        "src/telegram.rs",
    ] {
        assert!(
            !source(module).contains("TGS_TEST_"),
            "{module} contains a runtime test hook"
        );
    }
}
