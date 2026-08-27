#![allow(clippy::expect_used, clippy::panic)]
//! End-to-end process tests for CLI input, output, and local configuration.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
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
        .env("TGS_TOKEN", "not-a-real-token")
        .env("TGS_CHAT_ID", "1")
        .args(["--parse-mode", "html"]);
    command
}

fn get_me_server(request_count: usize) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = listener.local_addr().expect("address");
    let handle = std::thread::spawn(move || {
        let body = r#"{"ok":true,"result":{"id":42,"is_bot":true,"first_name":"Deploy","last_name":"Bot","username":"deploy_bot"}}"#;
        for _ in 0..request_count {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut bytes = Vec::new();
            let mut buffer = [0_u8; 1024];
            while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                let count = stream.read(&mut buffer).expect("read request");
                assert!(count > 0, "request ended before headers");
                bytes.extend_from_slice(&buffer[..count]);
            }
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .expect("response");
        }
    });
    (format!("http://{address}"), handle)
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

#[test]
fn bot_lifecycle_uses_custom_path_stdin_and_never_prints_token() {
    const TOKEN: &str = "000000:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let directory = temporary_directory("bot-lifecycle");
    let config = directory.join("custom.json");
    let (api_base, server) = get_me_server(2);

    let mut child = tgs()
        .env("TGS_TEST_API_BASE", &api_base)
        .args([
            "bot",
            "add",
            "ops",
            "--chat-id",
            "-100123",
            "--token-stdin",
            "--config",
        ])
        .arg(&config)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("add spawn");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(format!("{TOKEN}\n").as_bytes())
        .expect("token input");
    let add = child.wait_with_output().expect("add output");
    assert!(
        add.status.success(),
        "{}",
        String::from_utf8_lossy(&add.stderr)
    );
    assert!(!String::from_utf8_lossy(&add.stdout).contains(TOKEN));
    assert!(!String::from_utf8_lossy(&add.stderr).contains(TOKEN));

    let list = tgs()
        .args(["bot", "list", "--config"])
        .arg(&config)
        .output()
        .expect("list");
    assert_eq!(String::from_utf8_lossy(&list.stdout), "ops (default)\n");

    let info = tgs()
        .env("TGS_TEST_API_BASE", &api_base)
        .args(["bot", "info", "--config"])
        .arg(&config)
        .output()
        .expect("info");
    let info_output = String::from_utf8_lossy(&info.stdout);
    assert!(
        info.status.success(),
        "{}",
        String::from_utf8_lossy(&info.stderr)
    );
    assert!(info_output.contains("bot_id: 42"));
    assert!(info_output.contains("name: Deploy Bot"));
    assert!(info_output.contains("username: @deploy_bot"));
    assert!(!info_output.contains(TOKEN));

    let use_profile = tgs()
        .args(["bot", "use", "ops", "--config"])
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

    server.join().expect("server");
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
