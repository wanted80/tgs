#![allow(clippy::expect_used, clippy::panic)]
//! Offline local-server tests for Telegram transport behavior.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tgs_cli::bot::{BotChatLookup, BotIdentityLookup};
use tgs_cli::cli::SendArgs;
use tgs_cli::split::ParseMode;
use tgs_cli::telegram::{SendOptions, Sleeper, TelegramClient};
use tgs_cli::{Environment, send_message_with_base};

#[derive(Clone, Default)]
struct RecordingSleeper(Arc<Mutex<Vec<Duration>>>);

impl Sleeper for RecordingSleeper {
    fn sleep(&self, duration: Duration) {
        self.0.lock().expect("sleep lock").push(duration);
    }
}

struct TestServer {
    base: String,
    bodies: Arc<Mutex<Vec<String>>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(handle) = self.thread.take() {
            handle.join().expect("server thread");
        }
    }
}

fn server(responses: Vec<(&str, &str)>) -> TestServer {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = listener.local_addr().expect("address");
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&bodies);
    let mut responses: VecDeque<(String, String)> = responses
        .into_iter()
        .map(|(status, body)| (status.to_owned(), body.to_owned()))
        .collect();
    let handle = thread::spawn(move || {
        while let Some((status, body)) = responses.pop_front() {
            let (mut stream, _) = listener.accept().expect("accept");
            let request_body = read_request_body(&mut stream);
            captured.lock().expect("body lock").push(request_body);
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .expect("respond");
        }
    });
    TestServer {
        base: format!("http://{address}"),
        bodies,
        thread: Some(handle),
    }
}

fn read_request_body(stream: &mut std::net::TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 1024];
    let header_end = loop {
        let count = stream.read(&mut buffer).expect("read request");
        assert!(count > 0, "connection closed before headers");
        bytes.extend_from_slice(&buffer[..count]);
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
    };
    let headers = String::from_utf8_lossy(&bytes[..header_end]);
    let content_length = headers
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(str::trim)
                .map(str::parse::<usize>)
        })
        .transpose()
        .expect("content length number")
        .unwrap_or(0);
    while bytes.len() - header_end < content_length {
        let count = stream.read(&mut buffer).expect("read body");
        assert!(count > 0, "connection closed before body");
        bytes.extend_from_slice(&buffer[..count]);
    }
    String::from_utf8(bytes[header_end..header_end + content_length].to_vec()).expect("utf8 body")
}

fn options() -> SendOptions {
    SendOptions {
        parse_mode: ParseMode::Plain,
        disable_notification: false,
        disable_web_page_preview: false,
        protect_content: false,
        message_thread_id: None,
    }
}

fn send_environment(source: &str, device: &str) -> Environment {
    Environment::from_values([
        (
            "TGS_CONFIG".to_owned(),
            "missing-test-config.json".to_owned(),
        ),
        ("TGS_PROFILE".to_owned(), "test".to_owned()),
        (
            "TGS_TOKEN".to_owned(),
            "000000:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
        ),
        ("TGS_CHAT_ID".to_owned(), "1".to_owned()),
        ("TGS_SOURCE".to_owned(), source.to_owned()),
        ("TGS_DEVICE".to_owned(), device.to_owned()),
    ])
}

#[test]
fn sends_typed_json_and_reads_message_id() {
    let server = server(vec![(
        "200 OK",
        r#"{"ok":true,"result":{"message_id":42}}"#,
    )]);
    let client = TelegramClient::new(&server.base, 2).expect("client");
    let ids = client
        .send_chunks("token", "@alerts", &["hello".to_owned()], &options())
        .expect("send");
    assert_eq!(ids, [42]);
    let body: serde_json::Value =
        serde_json::from_str(server.bodies.lock().expect("bodies").first().expect("body"))
            .expect("json");
    assert_eq!(body["chat_id"], "@alerts");
    assert_eq!(body["text"], "hello");
    assert!(body.get("parse_mode").is_none());
}

#[test]
fn send_path_adds_deterministic_label_to_telegram_json() {
    let server = server(vec![(
        "200 OK",
        r#"{"ok":true,"result":{"message_id":42}}"#,
    )]);
    let report = send_message_with_base(
        &SendArgs::default(),
        "Task completed successfully",
        &send_environment("codex", "workstation-01"),
        &server.base,
    )
    .expect("send");
    assert_eq!(report.message_ids, [42]);
    let body: serde_json::Value =
        serde_json::from_str(server.bodies.lock().expect("bodies").first().expect("body"))
            .expect("json");
    assert_eq!(
        body["text"],
        "[codex · workstation-01]\nTask completed successfully"
    );
}

#[test]
fn no_label_preserves_the_original_message_and_ignores_identity_environment() {
    let server = server(vec![(
        "200 OK",
        r#"{"ok":true,"result":{"message_id":43}}"#,
    )]);
    let environment = Environment::from_values([
        (
            "TGS_CONFIG".to_owned(),
            "missing-test-config.json".to_owned(),
        ),
        ("TGS_PROFILE".to_owned(), "test".to_owned()),
        (
            "TGS_TOKEN".to_owned(),
            "000000:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
        ),
        ("TGS_CHAT_ID".to_owned(), "1".to_owned()),
        ("TGS_SOURCE".to_owned(), " invalid ".to_owned()),
        ("TGS_DEVICE".to_owned(), "bad\nname".to_owned()),
    ]);
    send_message_with_base(
        &SendArgs {
            no_label: true,
            ..SendArgs::default()
        },
        "[original]\n<&>",
        &environment,
        &server.base,
    )
    .expect("send unchanged");
    let body: serde_json::Value =
        serde_json::from_str(server.bodies.lock().expect("bodies").first().expect("body"))
            .expect("json");
    assert_eq!(body["text"], "[original]\n<&>");
}

#[test]
fn invalid_identity_and_chunk_cap_fail_before_network_activity() {
    let server = server(Vec::new());
    let invalid = send_message_with_base(
        &SendArgs::default(),
        "hello",
        &send_environment(" bad ", "runner"),
        &server.base,
    )
    .expect_err("invalid label");
    assert_eq!(invalid.exit_code(), 2);

    let header_length = "[ci · runner]\n".chars().count();
    let oversized = "x".repeat(4096 - header_length + 1);
    let capped = send_message_with_base(
        &SendArgs {
            max_chunks: 1,
            ..SendArgs::default()
        },
        &oversized,
        &send_environment("ci", "runner"),
        &server.base,
    )
    .expect_err("chunk cap");
    assert!(capped.message().contains("--max-chunks"));
    assert!(server.bodies.lock().expect("bodies").is_empty());
}

#[test]
fn retries_only_429_twice_and_honors_retry_after() {
    let server = server(vec![
        (
            "429 Too Many Requests",
            r#"{"ok":false,"error_code":429,"parameters":{"retry_after":2}}"#,
        ),
        (
            "429 Too Many Requests",
            r#"{"ok":false,"error_code":429,"parameters":{"retry_after":3}}"#,
        ),
        ("200 OK", r#"{"ok":true,"result":{"message_id":9}}"#),
    ]);
    let sleeper = RecordingSleeper::default();
    let client = TelegramClient::with_sleeper(&server.base, 2, sleeper.clone()).expect("client");
    let result = client.send_chunks("token", "1", &["hello".to_owned()], &options());
    assert_eq!(result.expect("retry success"), [9]);
    assert_eq!(
        *sleeper.0.lock().expect("sleeps"),
        [Duration::from_secs(2), Duration::from_secs(3)]
    );
}

#[test]
fn does_not_retry_server_errors() {
    let server = server(vec![(
        "500 Internal Server Error",
        r#"{"ok":false,"error_code":500,"description":"down"}"#,
    )]);
    let client = TelegramClient::new(&server.base, 2).expect("client");
    let error = client
        .send_chunks("token", "1", &["hello".to_owned()], &options())
        .expect_err("server failure");
    assert!(error.message().contains("500"));
    assert_eq!(server.bodies.lock().expect("bodies").len(), 1);
}

#[test]
fn rejects_retry_waits_above_cap() {
    let server = server(vec![(
        "429 Too Many Requests",
        r#"{"ok":false,"parameters":{"retry_after":61}}"#,
    )]);
    let client = TelegramClient::new(&server.base, 2).expect("client");
    let error = client
        .send_chunks("token", "1", &["hello".to_owned()], &options())
        .expect_err("wait cap");
    assert!(error.message().contains("maximum allowed"));
}

#[test]
fn malformed_response_is_actionable() {
    let server = server(vec![("200 OK", "not-json")]);
    let client = TelegramClient::new(&server.base, 2).expect("client");
    let error = client
        .send_chunks("token", "1", &["hello".to_owned()], &options())
        .expect_err("malformed");
    assert!(error.message().contains("unreadable response"));
}

#[test]
fn reports_partial_delivery() {
    let server = server(vec![
        ("200 OK", r#"{"ok":true,"result":{"message_id":10}}"#),
        (
            "400 Bad Request",
            r#"{"ok":false,"error_code":400,"description":"bad chunk"}"#,
        ),
    ]);
    let client = TelegramClient::new(&server.base, 2).expect("client");
    let error = client
        .send_chunks(
            "token",
            "1",
            &["first".to_owned(), "second".to_owned()],
            &options(),
        )
        .expect_err("partial");
    assert_eq!(error.status(), "partial");
    assert_eq!(error.delivered_message_ids(), [10]);
}

#[test]
fn redacts_token_from_api_descriptions() {
    let token = "123:super-secret";
    let body = format!(r#"{{"ok":false,"error_code":400,"description":"invalid {token}"}}"#);
    let server = server(vec![("400 Bad Request", &body)]);
    let client = TelegramClient::new(&server.base, 2).expect("client");
    let error = client
        .send_chunks(token, "1", &["hello".to_owned()], &options())
        .expect_err("api error");
    assert!(!error.to_string().contains(token));
    assert!(error.to_string().contains("[REDACTED]"));
}

#[test]
fn timeout_is_reported_without_an_automatic_retry() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = listener.local_addr().expect("address");
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let _body = read_request_body(&mut stream);
        thread::sleep(Duration::from_millis(1200));
    });
    let client = TelegramClient::new(&format!("http://{address}"), 1).expect("client");
    let error = client
        .send_chunks("token", "1", &["hello".to_owned()], &options())
        .expect_err("timeout");
    assert!(error.message().contains("network or timeout"));
    handle.join().expect("server thread");
}

#[test]
fn get_me_returns_live_identity() {
    let server = server(vec![(
        "200 OK",
        r#"{"ok":true,"result":{"id":42,"is_bot":true,"first_name":"Deploy","last_name":"Bot","username":"deploy_bot"}}"#,
    )]);
    let client = TelegramClient::new(&server.base, 2).expect("client");
    let identity = client.get_me("token").expect("identity");
    assert_eq!(identity.id, 42);
    assert_eq!(identity.display_name, "Deploy Bot");
    assert_eq!(identity.username.as_deref(), Some("deploy_bot"));
}

#[test]
fn get_updates_returns_deduplicated_safe_recent_chats() {
    let server = server(vec![(
        "200 OK",
        r#"{
            "ok": true,
            "result": [
                {"update_id":1,"message":{"chat":{"id":42,"type":"private","first_name":"Carlo","last_name":"Rossi","username":"carlo"}}},
                {"update_id":2,"edited_message":{"chat":{"id":42,"type":"private","first_name":"Carlo","last_name":"Rossi","username":"carlo"}}},
                {"update_id":3,"channel_post":{"chat":{"id":-100123,"type":"channel","title":"Alerts","username":"alerts"}}},
                {"update_id":4,"my_chat_member":{"chat":{"id":-20,"type":"group","title":"Build team"}}},
                {"update_id":5,"poll":{"id":"ignored"}}
            ]
        }"#,
    )]);
    let client = TelegramClient::new(&server.base, 2).expect("client");
    let chats = client.recent_chats("token").expect("recent chats");
    assert_eq!(chats.len(), 3);
    assert_eq!(chats[0].id, -100123);
    assert_eq!(chats[0].kind, "channel");
    assert_eq!(chats[0].name, "Alerts (@alerts)");
    assert_eq!(chats[1].id, -20);
    assert_eq!(chats[1].name, "Build team");
    assert_eq!(chats[2].id, 42);
    assert_eq!(chats[2].name, "Carlo Rossi (@carlo)");
}

#[test]
fn get_updates_rejects_output_control_characters_and_redacts_errors() {
    let malformed = server(vec![(
        "200 OK",
        "{\"ok\":true,\"result\":[{\"message\":{\"chat\":{\"id\":1,\"type\":\"private\",\"first_name\":\"bad\\nname\"}}}]}",
    )]);
    let client = TelegramClient::new(&malformed.base, 2).expect("client");
    let error = client.recent_chats("token").expect_err("control character");
    assert_eq!(
        error.message(),
        "Telegram getUpdates returned invalid chat metadata"
    );

    let token = "000000:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let body =
        format!(r#"{{"ok":false,"error_code":409,"description":"webhook conflict for {token}"}}"#);
    let rejected = server(vec![("409 Conflict", &body)]);
    let client = TelegramClient::new(&rejected.base, 2).expect("client");
    let error = client.recent_chats(token).expect_err("API rejection");
    assert!(error.message().contains("409"));
    assert!(error.message().contains("[REDACTED]"));
    assert!(!error.message().contains(token));
}

#[test]
fn get_me_reports_rejected_tokens_without_exposing_them() {
    let token = "000000:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let body = format!(r#"{{"ok":false,"error_code":401,"description":"bad token {token}"}}"#);
    let server = server(vec![("401 Unauthorized", &body)]);
    let client = TelegramClient::new(&server.base, 2).expect("client");
    let error = client.get_me(token).expect_err("rejected");
    assert!(error.message().contains("401"));
    assert!(!error.message().contains(token));
}

#[test]
fn get_me_rejects_malformed_and_non_bot_responses() {
    let malformed = server(vec![("200 OK", "not-json")]);
    let client = TelegramClient::new(&malformed.base, 2).expect("client");
    assert!(
        client
            .get_me("token")
            .expect_err("malformed")
            .message()
            .contains("unreadable")
    );

    let non_bot = server(vec![(
        "200 OK",
        r#"{"ok":true,"result":{"id":42,"is_bot":false,"first_name":"Person"}}"#,
    )]);
    let client = TelegramClient::new(&non_bot.base, 2).expect("client");
    assert!(
        client
            .get_me("token")
            .expect_err("non-bot")
            .message()
            .contains("invalid bot identity")
    );

    let unsafe_name = server(vec![(
        "200 OK",
        "{\"ok\":true,\"result\":{\"id\":42,\"is_bot\":true,\"first_name\":\"line\\nbreak\"}}",
    )]);
    let client = TelegramClient::new(&unsafe_name.base, 2).expect("client");
    assert_eq!(
        client
            .get_me("token")
            .expect_err("unsafe display name")
            .message(),
        "Telegram getMe returned an invalid bot identity"
    );
}

#[test]
fn get_me_reports_http_failures() {
    let server = server(vec![(
        "503 Service Unavailable",
        r#"{"ok":false,"error_code":503,"description":"temporarily unavailable"}"#,
    )]);
    let client = TelegramClient::new(&server.base, 2).expect("client");
    let error = client.get_me("token").expect_err("HTTP failure");
    assert!(error.message().contains("503"));
}

#[test]
fn get_me_timeout_is_explicit() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = listener.local_addr().expect("address");
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let _body = read_request_body(&mut stream);
        thread::sleep(Duration::from_millis(1200));
    });
    let client = TelegramClient::new(&format!("http://{address}"), 1).expect("client");
    let error = client.get_me("token").expect_err("timeout");
    assert!(error.message().contains("network or timeout"));
    handle.join().expect("server thread");
}
