#![allow(clippy::expect_used, clippy::panic)]
//! Offline local-server tests for Telegram transport behavior.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tgs_cli::bot::{BotChatLookup, BotIdentityLookup};
use tgs_cli::cli::SendArgs;
use tgs_cli::split::ParseMode;
use tgs_cli::telegram::{SendOptions, Sleeper, TelegramClient};
use tgs_cli::{Environment, send_message_with_base};

mod support;
use support::{server, stalled_server};

#[derive(Clone, Default)]
struct RecordingSleeper(Arc<Mutex<Vec<Duration>>>);

impl Sleeper for RecordingSleeper {
    fn sleep(&self, duration: Duration) {
        self.0.lock().expect("sleep lock").push(duration);
    }
}

#[test]
fn server_shutdown_does_not_require_all_expected_requests() {
    drop(server(vec![("200 OK", r#"{"ok":true}"#)]));
    drop(stalled_server());
}

#[test]
fn server_shutdown_interrupts_an_incomplete_request() {
    use std::io::Write;
    use std::net::TcpStream;
    let server = stalled_server();
    let mut connection =
        TcpStream::connect(server.base.trim_start_matches("http://")).expect("connect");
    connection
        .write_all(b"POST /sendMessage HTTP/1.1\r\n")
        .expect("partial headers");
    server.wait_for_connection();
    drop(server);
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
    let server = stalled_server();
    let client = TelegramClient::new(&server.base, 1).expect("client");
    let error = client
        .send_chunks("token", "1", &["hello".to_owned()], &options())
        .expect_err("timeout");
    assert!(error.message().contains("request timed out"));
    assert_eq!(
        server.methods.lock().expect("methods").as_slice(),
        ["sendMessage"]
    );
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
    let server = stalled_server();
    let client = TelegramClient::new(&server.base, 1).expect("client");
    let error = client.get_me("token").expect_err("timeout");
    assert!(error.message().contains("request timed out"));
    assert_eq!(
        server.methods.lock().expect("methods").as_slice(),
        ["getMe"]
    );
}

#[test]
fn pairing_ignores_unrelated_edited_wrong_chat_and_bot_messages() {
    let payload = serde_json::json!({"ok":true,"result":[
        {"message":{"text":"/start old-code","from":{"is_bot":false},"chat":{"id":1,"type":"private"}}},
        {"edited_message":{"text":"/start current-code","from":{"is_bot":false},"chat":{"id":1,"type":"private"}}},
        {"message":{"text":"/start current-code","from":{"is_bot":true},"chat":{"id":1,"type":"private"}}},
        {"message":{"text":"/start current-code","from":{"is_bot":false},"chat":{"id":-1,"type":"group"}}},
        {"message":{"text":"/start@someone_else current-code","from":{"is_bot":false},"chat":{"id":1,"type":"private"}}},
        {"message":{"text":"/start current-code extra","from":{"is_bot":false},"chat":{"id":1,"type":"private"}}}
    ]}).to_string();
    let server = server(vec![("200 OK", &payload)]);
    let client = TelegramClient::new(&server.base, 2).expect("client");
    assert!(
        client
            .find_pairing("token", "current-code", "review_bot", false)
            .expect("scan")
            .is_none()
    );
    assert_eq!(server.bodies.lock().expect("body")[0], "{}");
}

#[test]
fn pairing_accepts_only_explicit_human_senders() {
    for (sender, accepted) in [
        (None, false),
        (Some(serde_json::Value::Null), false),
        (Some(serde_json::json!({"is_bot": true})), false),
        (Some(serde_json::json!({"is_bot": false})), true),
    ] {
        let mut message = serde_json::json!({
            "text": "/start current-code",
            "chat": {"id": 7, "type": "private", "first_name": "Person"}
        });
        if let Some(sender) = sender {
            message["from"] = sender;
        }
        let payload = serde_json::json!({"ok": true, "result": [{"message": message}]}).to_string();
        let server = server(vec![("200 OK", &payload)]);
        let client = TelegramClient::new(&server.base, 2).expect("client");
        let chat = client
            .find_pairing("token", "current-code", "review_bot", false)
            .expect("scan");
        assert_eq!(chat.map(|chat| chat.id), accepted.then_some(7));
        assert_eq!(server.bodies.lock().expect("body")[0], "{}");
    }
}

#[test]
fn pairing_rejects_ambiguous_chats_and_full_queues_without_consuming_updates() {
    let updates = [1, 2].map(|id| serde_json::json!({"message":{"text":"/start code","from":{"is_bot":false},"chat":{"id":id,"type":"private"}}}));
    let ambiguous = serde_json::json!({"ok":true,"result":updates}).to_string();
    let full = serde_json::json!({"ok":true,"result":vec![serde_json::json!({});100]}).to_string();
    for (body, expected) in [
        (ambiguous, "multiple chats"),
        (full, "pending-update queue"),
    ] {
        let server = server(vec![("200 OK", &body)]);
        let client = TelegramClient::new(&server.base, 2).expect("client");
        let error = client
            .find_pairing("token", "code", "review_bot", false)
            .expect_err("reject");
        assert!(error.message().contains(expected));
        assert_eq!(server.bodies.lock().expect("body")[0], "{}");
    }
}

#[test]
fn setup_transport_rejects_malformed_metadata_and_redacts_remote_errors() {
    for body in [
        r#"{"ok":true,"result":{"id":0,"type":"private"}}"#,
        r#"{"ok":true,"result":{"id":1,"type":"private","first_name":"bad\nname"}}"#,
        r#"{"ok":true}"#,
    ] {
        let server = server(vec![("200 OK", body)]);
        let client = TelegramClient::new(&server.base, 2).expect("client");
        assert!(client.inspect_chat("token", "1").is_err());
    }
    let server = server(vec![(
        "403 Forbidden",
        r#"{"ok":false,"description":"bad token-secret"}"#,
    )]);
    let client = TelegramClient::new(&server.base, 2).expect("client");
    let error = client.has_webhook("token-secret").expect_err("reject");
    assert_eq!(error.exit_code(), 4);
    assert!(!error.message().contains("token-secret"));
}

#[test]
fn successful_identities_and_every_update_variant_are_redacted_before_return() {
    let token = "000000:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let identity = serde_json::json!({"ok":true,"result":{
        "id":42,"is_bot":true,"first_name":token,"last_name":token,"username":"review_bot"
    }})
    .to_string();
    let server = server(vec![("200 OK", &identity)]);
    let result = TelegramClient::new(&server.base, 2)
        .expect("client")
        .get_me(token)
        .expect("identity");
    assert!(!format!("{result:?}").contains(token));
    assert_eq!(result.display_name, "[REDACTED] [REDACTED]");
    drop(server);
    let updates: Vec<_> = ["message", "edited_message", "channel_post", "edited_channel_post", "my_chat_member", "chat_member", "chat_join_request"]
        .into_iter().enumerate().map(|(index, field)| {
            serde_json::json!({field:{"chat":{"id":-(index as i64 + 1),"type":"group","title":token}}})
        }).collect();
    let response = serde_json::json!({"ok":true,"result":updates}).to_string();
    let server = support::server(vec![("200 OK", &response)]);
    let chats = TelegramClient::new(&server.base, 2)
        .expect("client")
        .recent_chats(token)
        .expect("chats");
    assert_eq!(chats.len(), 7);
    assert!(chats.iter().all(|chat| chat.name == "[REDACTED]"));
    assert!(!format!("{chats:?}").contains(token));
}

#[test]
fn pairing_accepts_default_or_message_filters_and_rejects_malformed_filters() {
    for result in [
        serde_json::json!({"url":""}),
        serde_json::json!({"url":"","allowed_updates":[]}),
        serde_json::json!({"url":"","allowed_updates":["message"]}),
        serde_json::json!({"url":"","allowed_updates":["callback_query","message"]}),
    ] {
        let response = serde_json::json!({"ok":true,"result":result}).to_string();
        let server = server(vec![("200 OK", &response)]);
        TelegramClient::new(&server.base, 2)
            .expect("client")
            .check_pairing("token")
            .expect("pairing available");
        assert_eq!(server.bodies.lock().expect("body").as_slice(), ["{}"]);
        assert_eq!(
            server.methods.lock().expect("methods").as_slice(),
            ["getWebhookInfo"]
        );
    }
    let server = server(vec![(
        "200 OK",
        r#"{"ok":true,"result":{"url":"","allowed_updates":"PRIVATE_MARKER"}}"#,
    )]);
    let error = TelegramClient::new(&server.base, 2)
        .expect("client")
        .check_pairing("token")
        .expect_err("malformed");
    assert_eq!(error.exit_code(), 4);
    assert!(!error.message().contains("PRIVATE_MARKER"));
}
