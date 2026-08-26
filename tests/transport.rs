use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tgs_cli::split::ParseMode;
use tgs_cli::telegram::{SendOptions, Sleeper, TelegramClient};

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
        .expect("content length header");
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
