#![allow(clippy::expect_used, clippy::panic)]
//! Complete command workflows using isolated configuration and local HTTP.

mod support;

use std::collections::VecDeque;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tgs_cli::Environment;
use tgs_cli::app::{Console, execute_with_base};
use tgs_cli::cli::{self, MessageSource};
use tgs_cli::config::{Config, Profile};
use tgs_cli::error::{ErrorKind, TgsError};

const TOKEN: &str = "000000:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const IDENTITY: &str = r#"{"ok":true,"result":{"id":42,"is_bot":true,"first_name":"通知 Bot","username":"review_bot"}}"#;
const CHAT: &str = r#"{"ok":true,"result":{"id":7,"type":"private","first_name":"Carlo"}}"#;
const NO_WEBHOOK: &str = r#"{"ok":true,"result":{"url":""}}"#;
const SENT: &str = r#"{"ok":true,"result":{"message_id":123}}"#;

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "tgs-workflow-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("isolated directory");
        Self(path)
    }
    fn config(&self) -> PathBuf {
        self.0.join("config.json")
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("cleanup");
    }
}

struct Terminal {
    output: Arc<Mutex<Vec<u8>>>,
    answers: VecDeque<String>,
    token: String,
    stdin: String,
    broken_output: bool,
}

impl Terminal {
    fn new(answers: &[&str]) -> Self {
        Self {
            output: Arc::new(Mutex::new(Vec::new())),
            answers: answers.iter().map(|s| (*s).to_owned()).collect(),
            token: TOKEN.to_owned(),
            stdin: "hello".to_owned(),
            broken_output: false,
        }
    }
    fn text(&self) -> String {
        String::from_utf8(self.output.lock().expect("output").clone()).expect("utf8")
    }
}

impl Write for Terminal {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.broken_output {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        self.output.lock().expect("output").extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Console for Terminal {
    fn is_interactive(&self) -> bool {
        true
    }
    fn read_message(
        &mut self,
        source: Option<&MessageSource>,
        limit: usize,
    ) -> Result<Option<String>, TgsError> {
        let result = match source {
            Some(MessageSource::Message(s) | MessageSource::Positional(s)) => s.clone(),
            Some(MessageSource::File(path)) => {
                return tgs_cli::input::read_text(
                    fs::File::open(path).expect("message fixture"),
                    limit,
                    ErrorKind::Config,
                    "message",
                )
                .map(Some);
            }
            None => self.stdin.clone(),
        };
        tgs_cli::input::read_text(result.as_bytes(), limit, ErrorKind::Usage, "message").map(Some)
    }
    fn read_token(&mut self, _from_stdin: bool) -> Result<String, TgsError> {
        Ok(self.token.clone())
    }
    fn prompt(&mut self, message: &str) -> Result<String, TgsError> {
        self.write_all(message.as_bytes())
            .map_err(|_| TgsError::config("output failed"))?;
        self.answers
            .pop_front()
            .ok_or_else(|| TgsError::usage("input closed"))
    }
}

fn environment(path: &Path, extra: &[(&str, &str)]) -> Environment {
    Environment::from_values(
        std::iter::once((
            "TGS_CONFIG".to_owned(),
            path.to_str().expect("path").to_owned(),
        ))
        .chain(
            extra
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
        ),
    )
}

fn execute(
    arguments: &[&str],
    env: &Environment,
    terminal: &mut Terminal,
    base: &str,
) -> Result<(), TgsError> {
    let command = cli::parse(arguments.iter().map(|s| (*s).to_owned()))?;
    execute_with_base(command, env, terminal, base)
}

fn save_profile(path: &Path, name: &str, token: &str, chat: &str) {
    let mut config = Config::default();
    config.profiles.insert(
        name.to_owned(),
        Profile {
            token: token.to_owned(),
            chat_id: Some(chat.to_owned()),
        },
    );
    config.save(path).expect("save fixture");
}

#[test]
fn all_message_sources_and_success_modes_use_the_complete_command_path() {
    let directory = Directory::new();
    let path = directory.config();
    let message_file = directory.0.join("message.txt");
    fs::write(&message_file, "hello").expect("message fixture");
    let env = environment(&path, &[("TGS_TOKEN", TOKEN), ("TGS_CHAT_ID", "7")]);
    for mode in ["silent", "text", "json"] {
        for source in [
            vec!["hello"],
            vec!["--message", "hello"],
            vec!["--file", message_file.to_str().expect("path")],
            vec![],
        ] {
            let server = support::server(vec![("200 OK", SENT)]);
            let mut terminal = Terminal::new(&[]);
            let mut arguments = vec!["--no-label", "--output", mode];
            arguments.extend(source);
            execute(&arguments, &env, &mut terminal, &server.base).expect("send");
            let output = terminal.text();
            match mode {
                "silent" => assert!(output.is_empty()),
                "text" => assert!(output.contains("sent 1 chunk(s) to 7 using profile default")),
                _ => {
                    let report: serde_json::Value = serde_json::from_str(&output).expect("JSON");
                    assert_eq!(report["message_ids"], serde_json::json!([123]));
                    assert_eq!(report["profile"], "default");
                }
            }
            let bodies = server.bodies.lock().expect("bodies");
            assert_eq!(bodies.len(), 1);
            let body: serde_json::Value = serde_json::from_str(&bodies[0]).expect("request");
            assert_eq!(body["text"], "hello");
            assert!(!output.contains(TOKEN));
        }
    }
}

#[test]
fn a_single_profile_needs_no_default_and_overrides_have_documented_precedence() {
    let directory = Directory::new();
    let path = directory.config();
    save_profile(&path, "only", TOKEN, "1");
    for (extra, args, destination) in [
        (vec![], vec!["--no-label", "hello"], "1"),
        (vec![("TGS_CHAT_ID", "2")], vec!["--no-label", "hello"], "2"),
        (
            vec![("TGS_CHAT_ID", "2")],
            vec!["--no-label", "--chat-id", "3", "hello"],
            "3",
        ),
    ] {
        let server = support::server(vec![("200 OK", SENT)]);
        let env = environment(&path, &extra);
        execute(&args, &env, &mut Terminal::new(&[]), &server.base).expect("send");
        let bodies = server.bodies.lock().expect("bodies");
        let request: serde_json::Value = serde_json::from_str(&bodies[0]).expect("JSON");
        assert_eq!(request["chat_id"], destination);
    }
}

#[test]
fn multiple_profiles_without_a_default_require_an_explicit_choice() {
    let directory = Directory::new();
    let path = directory.config();
    save_profile(&path, "one", TOKEN, "1");
    let mut config = Config::load(&path).expect("load");
    config.profiles.insert(
        "two".to_owned(),
        Profile {
            token: TOKEN.to_owned(),
            chat_id: Some("2".to_owned()),
        },
    );
    config.save(&path).expect("save");
    let server = support::server(vec![]);
    let error = execute(
        &["hello"],
        &environment(&path, &[]),
        &mut Terminal::new(&[]),
        &server.base,
    )
    .expect_err("ambiguous");
    assert!(error.message().contains("multiple profiles"));
    assert!(server.methods.lock().expect("methods").is_empty());
}

#[test]
fn successful_send_followed_by_broken_output_retains_delivery_evidence() {
    let directory = Directory::new();
    let env = environment(
        &directory.config(),
        &[("TGS_TOKEN", TOKEN), ("TGS_CHAT_ID", "7")],
    );
    let server = support::server(vec![("200 OK", SENT)]);
    let mut terminal = Terminal::new(&[]);
    terminal.broken_output = true;
    let error = execute(
        &["--output", "json", "hello"],
        &env,
        &mut terminal,
        &server.base,
    )
    .expect_err("output failed");
    assert_eq!(error.exit_code(), 3);
    assert_eq!(error.delivered_message_ids(), [123]);
    assert_eq!(
        server.methods.lock().expect("methods").as_slice(),
        ["sendMessage"]
    );
}

#[test]
fn explicit_destination_setup_saves_only_after_confirmation_and_can_test_delivery() {
    let directory = Directory::new();
    let path = directory.config();
    let observed_path = path.clone();
    let server = support::dynamic_server(3, move |index, _| {
        if index < 2 {
            assert!(
                !observed_path.exists(),
                "no partial config before confirmation"
            );
        } else {
            assert!(observed_path.exists(), "save before test delivery");
        }
        (
            "200 OK".to_owned(),
            [IDENTITY, CHAT, SENT][index].to_owned(),
        )
    });
    let mut terminal = Terminal::new(&["yes", "yes"]);
    execute(
        &["setup", "--chat-id", "7"],
        &environment(&path, &[]),
        &mut terminal,
        &server.base,
    )
    .expect("setup");
    let config = Config::load(&path).expect("complete config");
    assert_eq!(config.default_profile.as_deref(), Some("default"));
    assert_eq!(config.profiles["default"].chat_id.as_deref(), Some("7"));
    assert_eq!(config.profiles["default"].token, TOKEN);
    assert!(!terminal.text().contains(TOKEN));
    assert!(terminal.answers.is_empty());
    assert_eq!(
        server.methods.lock().expect("methods").as_slice(),
        ["getMe", "getChat", "sendMessage"]
    );
}

#[test]
fn cancel_and_input_eof_leave_existing_configuration_unchanged() {
    for answers in [vec!["yes", "no"], vec!["yes"]] {
        let directory = Directory::new();
        let path = directory.config();
        save_profile(&path, "only", TOKEN, "1");
        let original = fs::read(&path).expect("original");
        let server = support::server(vec![("200 OK", IDENTITY), ("200 OK", CHAT)]);
        let error = execute(
            &["setup", "--chat-id", "7"],
            &environment(&path, &[]),
            &mut Terminal::new(&answers),
            &server.base,
        )
        .expect_err("cancel/EOF");
        assert_eq!(error.exit_code(), 2);
        assert_eq!(fs::read(&path).expect("unchanged"), original);
        assert_eq!(server.methods.lock().expect("methods").len(), 2);
    }
}

#[test]
fn test_notification_failure_keeps_the_completed_configuration() {
    let directory = Directory::new();
    let path = directory.config();
    let server = support::server(vec![
        ("200 OK", IDENTITY),
        ("200 OK", CHAT),
        (
            "403 Forbidden",
            r#"{"ok":false,"description":"cannot send"}"#,
        ),
    ]);
    let error = execute(
        &["setup", "--chat-id", "7"],
        &environment(&path, &[]),
        &mut Terminal::new(&["yes", "yes"]),
        &server.base,
    )
    .expect_err("test rejected");
    assert_eq!(error.exit_code(), 4);
    assert_eq!(
        Config::load(&path).expect("saved").profiles["default"]
            .chat_id
            .as_deref(),
        Some("7")
    );
}

#[test]
fn existing_webhooks_are_reported_without_being_changed() {
    let directory = Directory::new();
    let path = directory.config();
    let server = support::server(vec![
        ("200 OK", IDENTITY),
        (
            "200 OK",
            r#"{"ok":true,"result":{"url":"https://example.test/private-secret"}}"#,
        ),
    ]);
    let mut terminal = Terminal::new(&[]);
    let error = execute(
        &["setup"],
        &environment(&path, &[]),
        &mut terminal,
        &server.base,
    )
    .expect_err("webhook");
    assert_eq!(error.exit_code(), 3);
    assert!(error.message().contains("setup --chat-id"));
    assert!(!error.message().contains("private-secret"));
    assert!(!path.exists());
    assert_eq!(
        server.methods.lock().expect("methods").as_slice(),
        ["getMe", "getWebhookInfo"]
    );
}

#[test]
fn pairing_uses_the_random_link_and_never_acknowledges_updates() {
    for group in [false, true] {
        let directory = Directory::new();
        let path = directory.config();
        let mut terminal = Terminal::new(&["", "yes", "no"]);
        let output = Arc::clone(&terminal.output);
        let server = support::dynamic_server(3, move |index, body| {
            let result = match index {
                0 => IDENTITY.to_owned(),
                1 => NO_WEBHOOK.to_owned(),
                _ => {
                    let payload: serde_json::Value = serde_json::from_str(body).expect("JSON");
                    assert!(
                        payload.as_object().expect("object").is_empty(),
                        "no offset, limit, or allowed_updates changes"
                    );
                    let text =
                        String::from_utf8(output.lock().expect("output").clone()).expect("utf8");
                    let link = text
                        .lines()
                        .find(|line| line.starts_with("https://t.me/review_bot?"))
                        .expect("pairing link");
                    let (parameter, code) = link.split_once('=').expect("code");
                    assert_eq!(code.len(), 32);
                    assert!(code.bytes().all(|b| b.is_ascii_hexdigit()));
                    assert!(parameter.ends_with(if group { "startgroup" } else { "start" }));
                    let chat = if group {
                        serde_json::json!({"id":-7,"type":"supergroup","title":"Build team"})
                    } else {
                        serde_json::json!({"id":7,"type":"private","first_name":"Carlo"})
                    };
                    serde_json::json!({"ok":true,"result":[{"message":{"text":format!("/start@review_bot {code}"),"from":{"is_bot":false},"chat":chat}}]}).to_string()
                }
            };
            ("200 OK".to_owned(), result)
        });
        let args = if group {
            vec!["setup", "--group", "--qr"]
        } else {
            vec!["setup"]
        };
        let extra = if group {
            vec![("TGS_CHAT_ID", "999")]
        } else {
            vec![]
        };
        execute(
            &args,
            &environment(&path, &extra),
            &mut terminal,
            &server.base,
        )
        .expect("pair");
        let config = Config::load(&path).expect("saved");
        assert_eq!(
            config.profiles["default"].chat_id.as_deref(),
            Some(if group { "-7" } else { "7" })
        );
        assert!(!terminal.text().contains(TOKEN));
        if group {
            assert!(terminal.text().contains('█'));
        }
        assert_eq!(
            server.methods.lock().expect("methods").as_slice(),
            ["getMe", "getWebhookInfo", "getUpdates"]
        );
    }
}

#[test]
fn pairing_retries_missing_messages_and_cancels_without_saving() {
    let directory = Directory::new();
    let path = directory.config();
    let server = support::server(vec![
        ("200 OK", IDENTITY),
        ("200 OK", NO_WEBHOOK),
        ("200 OK", r#"{"ok":true,"result":[]}"#),
    ]);
    let mut terminal = Terminal::new(&["", "q"]);
    let error = execute(
        &["setup"],
        &environment(&path, &[]),
        &mut terminal,
        &server.base,
    )
    .expect_err("cancel");
    assert_eq!(error.exit_code(), 2);
    assert!(!path.exists());
    assert!(terminal.text().contains("has not arrived"));
}

#[test]
fn status_checks_the_resolved_destination_without_sending_or_writing() {
    let directory = Directory::new();
    let path = directory.config();
    save_profile(&path, "only", TOKEN, "1");
    let original = fs::read(&path).expect("original");
    for output in ["text", "json"] {
        let server = support::server(vec![
            ("200 OK", IDENTITY),
            ("200 OK", CHAT),
            ("200 OK", NO_WEBHOOK),
        ]);
        let mut terminal = Terminal::new(&[]);
        execute(
            &["status", "--output", output],
            &environment(&path, &[("TGS_CHAT_ID", "7")]),
            &mut terminal,
            &server.base,
        )
        .expect("status");
        assert_eq!(fs::read(&path).expect("unchanged"), original);
        assert_eq!(
            server.methods.lock().expect("methods").as_slice(),
            ["getMe", "getChat", "getWebhookInfo"]
        );
        assert!(!terminal.text().contains(TOKEN));
        if output == "json" {
            let json: serde_json::Value = serde_json::from_str(&terminal.text()).expect("JSON");
            assert_eq!(json["profile"], "only");
            assert_eq!(json["destination"], "7");
            assert_eq!(json["delivery_tested"], false);
        }
    }
}

#[test]
fn setup_rejects_invalid_tokens_before_network_or_persistence() {
    let directory = Directory::new();
    let path = directory.config();
    let server = support::server(vec![]);
    let mut terminal = Terminal::new(&[]);
    terminal.token = "PRIVATE_INVALID_TOKEN".to_owned();
    let error = execute(
        &["setup", "--chat-id", "7"],
        &environment(&path, &[]),
        &mut terminal,
        &server.base,
    )
    .expect_err("token");
    assert_eq!(error.exit_code(), 2);
    assert!(!error.message().contains("PRIVATE_INVALID_TOKEN"));
    assert!(!path.exists());
    assert!(server.methods.lock().expect("methods").is_empty());
}

#[test]
fn setup_does_not_overwrite_edits_made_during_the_wizard() {
    let directory = Directory::new();
    let path = directory.config();
    let modified_path = path.clone();
    let server = support::dynamic_server(2, move |index, _| {
        if index == 1 {
            save_profile(&modified_path, "concurrent", TOKEN, "8");
        }
        (
            "200 OK".to_owned(),
            if index == 0 { IDENTITY } else { CHAT }.to_owned(),
        )
    });
    let error = execute(
        &["setup", "--chat-id", "7"],
        &environment(&path, &[]),
        &mut Terminal::new(&["yes"]),
        &server.base,
    )
    .expect_err("concurrent edit");
    assert!(error.message().contains("changed during this command"));
    let saved = Config::load(&path).expect("concurrent config");
    assert_eq!(saved.profiles.len(), 1);
    assert!(saved.profiles.contains_key("concurrent"));
}

#[test]
fn environment_token_is_persisted_only_after_explicit_consent() {
    let directory = Directory::new();
    let path = directory.config();
    let server = support::server(vec![]);
    let error = execute(
        &["setup", "--chat-id", "7"],
        &environment(&path, &[("TGS_TOKEN", TOKEN)]),
        &mut Terminal::new(&["no"]),
        &server.base,
    )
    .expect_err("decline");
    assert_eq!(error.exit_code(), 2);
    assert!(!path.exists());
    assert!(server.methods.lock().expect("methods").is_empty());
}

#[test]
fn bot_commands_redact_tokens_in_successful_telegram_metadata() {
    let directory = Directory::new();
    let path = directory.config();
    save_profile(&path, "only", TOKEN, "7");
    let identity = serde_json::json!({"ok":true,"result":{
        "id":42,"is_bot":true,"first_name":TOKEN,"username":"review_bot"
    }})
    .to_string();
    let chats = serde_json::json!({"ok":true,"result":[{"message":{"chat":{
        "id":-7,"type":"group","title":format!("Alerts {TOKEN}")
    }}}]})
    .to_string();
    for (arguments, response) in [
        (vec!["bot", "info"], identity.as_str()),
        (vec!["bot", "add", "only"], identity.as_str()),
        (vec!["bot", "create", "new"], identity.as_str()),
        (vec!["bot", "chats"], chats.as_str()),
    ] {
        let server = support::server(vec![("200 OK", response)]);
        let mut terminal = Terminal::new(&[]);
        execute(
            &arguments,
            &environment(&path, &[]),
            &mut terminal,
            &server.base,
        )
        .expect("command");
        assert!(!terminal.text().contains(TOKEN));
        assert!(terminal.text().contains("[REDACTED]"));
    }
}

#[test]
fn adding_a_bot_does_not_overwrite_a_concurrent_edit_during_get_me() {
    for action in ["add", "create"] {
        let directory = Directory::new();
        let path = directory.config();
        save_profile(&path, "original", TOKEN, "7");
        let concurrent = path.clone();
        let server = support::dynamic_server(1, move |_, _| {
            tgs_cli::config::set_profile(&concurrent, "concurrent", Some("8"), Some(TOKEN))
                .expect("concurrent edit");
            ("200 OK".to_owned(), IDENTITY.to_owned())
        });
        let error = execute(
            &["bot", action, "pending"],
            &environment(&path, &[]),
            &mut Terminal::new(&[]),
            &server.base,
        )
        .expect_err("conflict");
        assert_eq!(error.exit_code(), 3);
        assert!(error.message().contains("changed during this command"));
        let config = Config::load(&path).expect("result");
        assert!(config.profiles.contains_key("concurrent"));
        assert!(config.profiles.contains_key("original"));
        assert!(!config.profiles.contains_key("pending"));
    }
}

#[test]
fn incompatible_pairing_filters_fail_before_links_or_polling_without_changing_state() {
    let directory = Directory::new();
    let path = directory.config();
    let server = support::server(vec![
        ("200 OK", IDENTITY),
        (
            "200 OK",
            r#"{"ok":true,"result":{"url":"","allowed_updates":["callback_query","PRIVATE_MARKER"]}}"#,
        ),
    ]);
    let mut terminal = Terminal::new(&[]);
    let error = execute(
        &["setup"],
        &environment(&path, &[]),
        &mut terminal,
        &server.base,
    )
    .expect_err("filter");
    assert_eq!(error.exit_code(), 3);
    assert!(error.message().contains("allowed_updates"));
    assert!(error.message().contains("setup --chat-id"));
    assert!(!error.message().contains("PRIVATE_MARKER"));
    assert!(!terminal.text().contains("https://t.me/review_bot?"));
    assert!(terminal.answers.is_empty());
    assert!(!path.exists());
    assert_eq!(
        server.methods.lock().expect("methods").as_slice(),
        ["getMe", "getWebhookInfo"]
    );
}

#[test]
fn setup_rejects_the_reserved_lock_path_before_prompting_or_network() {
    let directory = Directory::new();
    let path = directory.0.join(".TGS-CONFIG.LOCK");
    let server = support::server(vec![]);
    let mut terminal = Terminal::new(&[]);
    let error = execute(
        &["setup"],
        &environment(&path, &[]),
        &mut terminal,
        &server.base,
    )
    .expect_err("reserved path");
    assert_eq!(error.exit_code(), 3);
    assert!(error.message().contains("reserved lock filename"));
    assert!(terminal.text().is_empty());
    assert!(!path.exists());
    assert!(server.methods.lock().expect("methods").is_empty());
}

#[test]
fn newline_terminated_messages_never_send_blank_chunks() {
    let directory = Directory::new();
    let path = directory.config();
    save_profile(&path, "only", TOKEN, "7");
    let server = support::dynamic_server(2, |_, body| {
        let request: serde_json::Value = serde_json::from_str(body).expect("request");
        assert!(!request["text"].as_str().expect("text").trim().is_empty());
        ("200 OK".to_owned(), SENT.to_owned())
    });
    let message = format!("{}\n", "a".repeat(4096));
    execute(
        &["--no-label", "--message", &message],
        &environment(&path, &[]),
        &mut Terminal::new(&[]),
        &server.base,
    )
    .expect("complete send");
    let bodies = server.bodies.lock().expect("bodies");
    let text: String = bodies
        .iter()
        .map(|body| {
            let request: serde_json::Value = serde_json::from_str(body).expect("request");
            request["text"].as_str().expect("text").to_owned()
        })
        .collect();
    assert_eq!(text, message);
}

#[test]
fn unsendable_whitespace_and_oversized_ids_fail_before_network_requests() {
    let directory = Directory::new();
    let path = directory.config();
    save_profile(&path, "only", TOKEN, "7");
    let server = support::server(vec![]);
    let message = format!("a{}", " ".repeat(8192));
    for args in [
        vec!["--no-label", "--message", &message],
        vec!["--message", &message],
        vec!["--chat-id", "9223372036854775808", "hello"],
    ] {
        let error = execute(
            &args,
            &environment(&path, &[]),
            &mut Terminal::new(&[]),
            &server.base,
        )
        .expect_err("preflight");
        assert_eq!(error.exit_code(), 2);
        assert!(error.delivered_message_ids().is_empty());
    }
    assert!(server.methods.lock().expect("methods").is_empty());
}

#[cfg(any(unix, windows))]
fn invalid_native_string() -> std::ffi::OsString {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(b"PRIVATE_MARKER\xff".to_vec())
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        std::ffi::OsString::from_wide(&[0xd800])
    }
}

#[cfg(any(unix, windows))]
#[test]
fn invalid_environment_encoding_never_falls_back_to_saved_credentials() {
    let directory = Directory::new();
    let path = directory.config();
    save_profile(&path, "only", TOKEN, "7");
    let server = support::server(vec![]);
    for key in [
        "TGS_CHAT_ID",
        "TGS_TOKEN",
        "TGS_PROFILE",
        "TGS_CONFIG",
        "TGS_SOURCE",
        "TGS_DEVICE",
    ] {
        let env = Environment::from_os_values([
            ("TGS_CONFIG".to_owned(), path.clone().into_os_string()),
            (key.to_owned(), invalid_native_string()),
        ]);
        let error = execute(&["hello"], &env, &mut Terminal::new(&[]), &server.base)
            .expect_err("invalid environment");
        assert_eq!(error.exit_code(), 3);
        assert_eq!(error.message(), format!("{key} must contain valid Unicode"));
        assert!(!format!("{error:?} {env:?}").contains("PRIVATE_MARKER"));
    }
    assert!(server.methods.lock().expect("methods").is_empty());
}

#[cfg(any(unix, windows))]
#[test]
fn explicit_overrides_and_no_label_ignore_unused_invalid_environment_values() {
    let directory = Directory::new();
    let path = directory.config();
    save_profile(&path, "only", TOKEN, "7");
    let env = Environment::from_os_values(
        [
            "TGS_CHAT_ID",
            "TGS_PROFILE",
            "TGS_CONFIG",
            "TGS_SOURCE",
            "TGS_DEVICE",
        ]
        .map(|key| (key.to_owned(), invalid_native_string())),
    );
    let server = support::server(vec![("200 OK", SENT)]);
    execute(
        &[
            "--config",
            path.to_str().expect("path"),
            "--profile",
            "only",
            "--chat-id",
            "8",
            "--no-label",
            "hello",
        ],
        &env,
        &mut Terminal::new(&[]),
        &server.base,
    )
    .expect("explicit selection");
    let bodies = server.bodies.lock().expect("bodies");
    let request: serde_json::Value = serde_json::from_str(&bodies[0]).expect("request");
    assert_eq!(request["chat_id"], "8");
    assert_eq!(request["text"], "hello");
}
