#![allow(clippy::expect_used, clippy::panic)]
//! End-to-end process tests for CLI input, output, and local configuration.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn temporary_directory(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("tgs-e2e-{name}-{}-{nonce}", std::process::id()));
    fs::create_dir_all(&path).expect("create test directory");
    path
}

fn tgs() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_tgs"));
    command.env_clear();

    #[cfg(windows)]
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system_root);
    }

    command
}

fn configured_tgs(config: &std::path::Path) -> Command {
    let mut command = tgs();
    command
        .env("TGS_CONFIG", config)
        .env("TGS_PROFILE", "test")
        .env("TGS_TOKEN", "000000:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        .env("TGS_CHAT_ID", "1")
        .args(["--parse-mode", "html"]);
    command
}

#[test]
fn config_lifecycle_never_displays_token() {
    let directory = temporary_directory("config");
    let config = directory.join("config.json");

    let init = tgs()
        .args(["config", "init", "ops", "--config"])
        .arg(&config)
        .output()
        .expect("init");
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );

    let mut child = tgs()
        .args([
            "config",
            "set",
            "ops",
            "--chat-id",
            "@alerts",
            "--token-stdin",
            "--config",
        ])
        .arg(&config)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("set spawn");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"000000:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n")
        .expect("token input");
    let set = child.wait_with_output().expect("set output");
    assert!(
        set.status.success(),
        "{}",
        String::from_utf8_lossy(&set.stderr)
    );

    let show = tgs()
        .args(["config", "show", "ops", "--config"])
        .arg(&config)
        .output()
        .expect("show");
    let stdout = String::from_utf8(show.stdout).expect("show utf8");
    assert!(stdout.contains("token: set"));
    assert!(!stdout.contains("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));

    let remove = tgs()
        .args(["config", "remove", "ops", "--config"])
        .arg(&config)
        .output()
        .expect("remove");
    assert!(remove.status.success());
    fs::remove_dir_all(directory).expect("cleanup");
}

#[test]
fn failure_output_is_versioned_json_with_exit_code() {
    let output = tgs()
        .args(["--message", ""])
        .output()
        .expect("run empty message");
    assert_eq!(output.status.code(), Some(2));
    let report: serde_json::Value = serde_json::from_slice(&output.stderr).expect("failure json");
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["status"], "error");
    assert_eq!(report["chunks_sent"], 0);
}

#[test]
fn help_and_version_report_the_package_version_consistently() {
    let help = tgs().arg("--help").output().expect("help");
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).expect("help utf8");
    assert!(help.starts_with(&format!("tgs {} —", env!("CARGO_PKG_VERSION"))));

    let nested_help = tgs()
        .args(["bot", "chats", "--help"])
        .output()
        .expect("nested help");
    assert!(nested_help.status.success());
    assert_eq!(nested_help.stdout, help.as_bytes());

    let version = tgs().arg("--version").output().expect("version");
    assert_eq!(
        String::from_utf8(version.stdout).expect("version utf8"),
        format!("tgs {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn every_message_source_reaches_shared_validation_without_network() {
    let directory = temporary_directory("sources");
    let config = directory.join("missing.json");
    let message_file = directory.join("message.txt");
    let message = "x".repeat(4097);
    fs::write(&message_file, &message).expect("message file");

    let positional = configured_tgs(&config)
        .arg(&message)
        .output()
        .expect("positional");
    assert_eq!(positional.status.code(), Some(2));

    let option = configured_tgs(&config)
        .args(["--message", &message])
        .output()
        .expect("message option");
    assert_eq!(option.status.code(), Some(2));

    let file = configured_tgs(&config)
        .arg("--file")
        .arg(&message_file)
        .output()
        .expect("file");
    assert_eq!(file.status.code(), Some(2));

    let mut child = configured_tgs(&config)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("stdin spawn");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(message.as_bytes())
        .expect("stdin message");
    let stdin = child.wait_with_output().expect("stdin output");
    assert_eq!(stdin.status.code(), Some(2));

    fs::remove_dir_all(directory).expect("cleanup");
}

#[test]
fn bot_local_lifecycle_uses_the_shared_configuration_operations() {
    let directory = temporary_directory("bot-lifecycle");
    let config = directory.join("custom.json");

    let init_ops = tgs()
        .args(["config", "init", "ops", "--config"])
        .arg(&config)
        .output()
        .expect("init ops");
    assert!(init_ops.status.success());
    let init_ci = tgs()
        .args(["config", "init", "ci", "--config"])
        .arg(&config)
        .output()
        .expect("init ci");
    assert!(init_ci.status.success());

    let list = tgs()
        .args(["bot", "list", "--config"])
        .arg(&config)
        .output()
        .expect("list");
    assert_eq!(String::from_utf8_lossy(&list.stdout), "ci\nops (default)\n");

    let use_profile = tgs()
        .args(["bot", "use", "ci", "--config"])
        .arg(&config)
        .output()
        .expect("use");
    assert!(use_profile.status.success());

    let remove = tgs()
        .args(["bot", "remove", "ops", "--config"])
        .arg(&config)
        .output()
        .expect("remove");
    assert!(remove.status.success());
    assert!(String::from_utf8_lossy(&remove.stdout).contains("Telegram bot still exists"));

    fs::remove_dir_all(directory).expect("cleanup");
}

#[test]
fn invalid_bot_token_is_rejected_without_echoing_it_or_contacting_telegram() {
    let token = "000000:not valid and still secret";
    let directory = temporary_directory("bot-invalid-token");
    let config = directory.join("config.json");
    let mut child = tgs()
        .args(["bot", "add", "ops", "--token-stdin", "--config"])
        .arg(&config)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("bot add spawn");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(format!("{token}\n").as_bytes())
        .expect("token input");
    let output = child.wait_with_output().expect("bot add output");
    assert_eq!(output.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&output.stdout).contains(token));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(token));
    assert!(!config.exists());
    fs::remove_dir_all(directory).expect("cleanup");
}

#[test]
fn bot_create_refuses_a_duplicate_before_instructions_or_input() {
    let directory = temporary_directory("bot-create-duplicate");
    let config = directory.join("config.json");
    let init = tgs()
        .args(["config", "init", "ops", "--config"])
        .arg(&config)
        .output()
        .expect("init");
    assert!(init.status.success());

    let create = tgs()
        .args(["bot", "create", "ops", "--config"])
        .arg(&config)
        .output()
        .expect("create");
    assert_eq!(create.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&create.stdout).contains("BotFather"));
    fs::remove_dir_all(directory).expect("cleanup");
}
