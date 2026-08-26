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
    command
}

fn configured_tgs(config: &std::path::Path) -> Command {
    let mut command = tgs();
    command
        .env("TGS_CONFIG", config)
        .env("TGS_PROFILE", "test")
        .env("TGS_TOKEN", "not-a-real-token")
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
        .write_all(b"123:top-secret\n")
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
    assert!(!stdout.contains("top-secret"));

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
