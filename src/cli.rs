use std::path::PathBuf;

use crate::error::TgsError;
use crate::split::ParseMode;

pub const HELP: &str = r#"tgs 0.1.0 — send Telegram text notifications

USAGE:
    tgs [send] [OPTIONS] [MESSAGE...]
    tgs config <COMMAND> [OPTIONS]

MESSAGE SOURCES (choose exactly one):
    MESSAGE...                 Positional words, joined with spaces
    -m, --message TEXT         Message text
    -f, --file PATH            Read message from a file
                               With no explicit source, piped stdin is read

SEND OPTIONS:
    --profile NAME             Select a named profile
    --chat-id ID               Override the destination only
    --config PATH              Override the configuration file
    --parse-mode MODE          plain, html, or markdown-v2 [default: plain]
    --silent                   Disable Telegram notification
    --no-preview               Disable link previews
    --protect                  Protect forwarded/saved content
    --topic ID                 Telegram message thread/topic ID
    --timeout SECONDS          Request timeout [default: 10]
    --max-chunks N             Maximum plain-text chunks [default: 10]
    --output MODE              silent, text, or json [default: silent]
    --verbose                  Alias for --output text
    -h, --help                 Print help
    -V, --version              Print version

CONFIG COMMANDS:
    init [PROFILE]             Create configuration and a profile
    set PROFILE [OPTIONS]      Update --chat-id and/or token
    list                       List profile names
    show [PROFILE]             Show non-secret profile metadata
    use PROFILE                Set the default profile
    remove PROFILE             Remove a profile
    path                       Print the active configuration path

CONFIG SET OPTIONS:
    --chat-id ID               Set the profile destination
    --token-stdin              Read the token from stdin (otherwise hidden prompt)
"#;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputMode {
    Silent,
    Text,
    Json,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MessageSource {
    Positional(String),
    Message(String),
    File(PathBuf),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SendArgs {
    pub source: Option<MessageSource>,
    pub profile: Option<String>,
    pub chat_id: Option<String>,
    pub config: Option<PathBuf>,
    pub parse_mode: ParseMode,
    pub silent: bool,
    pub no_preview: bool,
    pub protect: bool,
    pub topic: Option<i64>,
    pub timeout: u64,
    pub max_chunks: usize,
    pub output: OutputMode,
}

impl Default for SendArgs {
    fn default() -> Self {
        Self {
            source: None,
            profile: None,
            chat_id: None,
            config: None,
            parse_mode: ParseMode::Plain,
            silent: false,
            no_preview: false,
            protect: false,
            topic: None,
            timeout: 10,
            max_chunks: 10,
            output: OutputMode::Silent,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigArgs {
    pub config: Option<PathBuf>,
    pub action: ConfigAction,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigAction {
    Init {
        profile: String,
    },
    Set {
        profile: String,
        chat_id: Option<String>,
        token_stdin: bool,
    },
    List,
    Show {
        profile: Option<String>,
    },
    Use {
        profile: String,
    },
    Remove {
        profile: String,
    },
    Path,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    Send(SendArgs),
    Config(ConfigArgs),
    Help,
    Version,
}

pub fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Command, TgsError> {
    let mut values: Vec<String> = arguments.into_iter().collect();
    if matches!(values.first().map(String::as_str), Some("-h" | "--help"))
        || matches!(
            (
                values.first().map(String::as_str),
                values.get(1).map(String::as_str)
            ),
            (Some("send" | "config"), Some("-h" | "--help"))
        )
    {
        return Ok(Command::Help);
    }
    if matches!(values.first().map(String::as_str), Some("-V" | "--version"))
        || matches!(
            (
                values.first().map(String::as_str),
                values.get(1).map(String::as_str)
            ),
            (Some("send"), Some("-V" | "--version"))
        )
    {
        return Ok(Command::Version);
    }
    if values.first().map(String::as_str) == Some("config") {
        values.remove(0);
        return parse_config(&values).map(Command::Config);
    }
    if values.first().map(String::as_str) == Some("send") {
        values.remove(0);
    }
    parse_send(&values).map(Command::Send)
}

fn parse_send(values: &[String]) -> Result<SendArgs, TgsError> {
    let mut result = SendArgs::default();
    let mut positional = Vec::new();
    let mut index = 0;
    let mut options = true;
    while index < values.len() {
        let value = &values[index];
        if options && value == "--" {
            options = false;
            index += 1;
            continue;
        }
        if !options || !value.starts_with('-') || value == "-" {
            positional.push(value.clone());
            index += 1;
            continue;
        }
        match value.as_str() {
            "-h" | "--help" => return Err(TgsError::usage("use `tgs --help` for usage")),
            "-m" | "--message" => {
                let text = next_value(values, &mut index, value)?;
                set_source(&mut result.source, MessageSource::Message(text))?;
            }
            "-f" | "--file" => {
                let path = next_value(values, &mut index, value)?;
                set_source(&mut result.source, MessageSource::File(path.into()))?;
            }
            "--profile" => result.profile = Some(next_value(values, &mut index, value)?),
            "--chat-id" => result.chat_id = Some(next_value(values, &mut index, value)?),
            "--config" => result.config = Some(next_value(values, &mut index, value)?.into()),
            "--parse-mode" => {
                result.parse_mode = next_value(values, &mut index, value)?.parse()?;
            }
            "--silent" => result.silent = true,
            "--no-preview" => result.no_preview = true,
            "--protect" => result.protect = true,
            "--topic" => {
                result.topic = Some(parse_number(
                    &next_value(values, &mut index, value)?,
                    value,
                )?);
                if result.topic.is_some_and(|topic| topic <= 0) {
                    return Err(TgsError::usage("--topic must be greater than zero"));
                }
            }
            "--timeout" => {
                result.timeout = parse_number(&next_value(values, &mut index, value)?, value)?;
                if result.timeout == 0 {
                    return Err(TgsError::usage("--timeout must be greater than zero"));
                }
            }
            "--max-chunks" => {
                result.max_chunks = parse_number(&next_value(values, &mut index, value)?, value)?;
                if result.max_chunks == 0 {
                    return Err(TgsError::usage("--max-chunks must be greater than zero"));
                }
            }
            "--output" => {
                result.output = match next_value(values, &mut index, value)?.as_str() {
                    "silent" => OutputMode::Silent,
                    "text" => OutputMode::Text,
                    "json" => OutputMode::Json,
                    other => {
                        return Err(TgsError::usage(format!(
                            "invalid output mode `{other}`; expected silent, text, or json"
                        )));
                    }
                };
            }
            "--verbose" => result.output = OutputMode::Text,
            unknown => return Err(TgsError::usage(format!("unknown option `{unknown}`"))),
        }
        index += 1;
    }
    if !positional.is_empty() {
        set_source(
            &mut result.source,
            MessageSource::Positional(positional.join(" ")),
        )?;
    }
    Ok(result)
}

fn parse_config(values: &[String]) -> Result<ConfigArgs, TgsError> {
    let action_name = values
        .first()
        .ok_or_else(|| TgsError::usage("missing config command; use `tgs --help`"))?;
    let mut config = None;
    let mut positional = Vec::new();
    let mut chat_id = None;
    let mut token_stdin = false;
    let mut index = 1;
    while index < values.len() {
        match values[index].as_str() {
            "--config" => config = Some(next_value(values, &mut index, "--config")?.into()),
            "--chat-id" => chat_id = Some(next_value(values, &mut index, "--chat-id")?),
            "--token-stdin" => token_stdin = true,
            value if value.starts_with('-') => {
                return Err(TgsError::usage(format!("unknown option `{value}`")));
            }
            value => positional.push(value.to_owned()),
        }
        index += 1;
    }

    let one_profile = |command: &str| -> Result<String, TgsError> {
        if positional.len() != 1 {
            return Err(TgsError::usage(format!(
                "config {command} requires exactly one profile name"
            )));
        }
        Ok(positional[0].clone())
    };
    let no_set_flags = || -> Result<(), TgsError> {
        if chat_id.is_some() || token_stdin {
            Err(TgsError::usage(
                "--chat-id and --token-stdin are valid only with `config set`",
            ))
        } else {
            Ok(())
        }
    };

    let action = match action_name.as_str() {
        "init" => {
            no_set_flags()?;
            if positional.len() > 1 {
                return Err(TgsError::usage("config init accepts at most one profile"));
            }
            ConfigAction::Init {
                profile: positional
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "default".to_owned()),
            }
        }
        "set" => ConfigAction::Set {
            profile: one_profile("set")?,
            chat_id,
            token_stdin,
        },
        "list" => {
            no_set_flags()?;
            ensure_no_positionals(&positional, "list")?;
            ConfigAction::List
        }
        "show" => {
            no_set_flags()?;
            if positional.len() > 1 {
                return Err(TgsError::usage("config show accepts at most one profile"));
            }
            ConfigAction::Show {
                profile: positional.first().cloned(),
            }
        }
        "use" => {
            no_set_flags()?;
            ConfigAction::Use {
                profile: one_profile("use")?,
            }
        }
        "remove" => {
            no_set_flags()?;
            ConfigAction::Remove {
                profile: one_profile("remove")?,
            }
        }
        "path" => {
            no_set_flags()?;
            ensure_no_positionals(&positional, "path")?;
            ConfigAction::Path
        }
        unknown => {
            return Err(TgsError::usage(format!(
                "unknown config command `{unknown}`"
            )));
        }
    };
    Ok(ConfigArgs { config, action })
}

fn next_value(values: &[String], index: &mut usize, option: &str) -> Result<String, TgsError> {
    *index += 1;
    values
        .get(*index)
        .cloned()
        .ok_or_else(|| TgsError::usage(format!("missing value for {option}")))
}

fn set_source(target: &mut Option<MessageSource>, value: MessageSource) -> Result<(), TgsError> {
    if target.is_some() {
        return Err(TgsError::usage("choose exactly one message source"));
    }
    *target = Some(value);
    Ok(())
}

fn parse_number<T: std::str::FromStr>(value: &str, option: &str) -> Result<T, TgsError> {
    value
        .parse()
        .map_err(|_| TgsError::usage(format!("invalid number for {option}: `{value}`")))
}

fn ensure_no_positionals(values: &[String], command: &str) -> Result<(), TgsError> {
    if values.is_empty() {
        Ok(())
    } else {
        Err(TgsError::usage(format!(
            "config {command} does not accept arguments"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn send_is_optional_and_positional_words_are_joined() {
        let plain = parse(args(&["hello", "world"]));
        let explicit = parse(args(&["send", "hello", "world"]));
        assert_eq!(plain.expect("plain"), explicit.expect("explicit"));
    }

    #[test]
    fn rejects_two_message_sources() {
        let error = parse(args(&["-m", "one", "two"])).expect_err("must reject");
        assert_eq!(error.exit_code(), 2);
    }

    #[test]
    fn parses_all_send_options() {
        let command = parse(args(&[
            "send",
            "-m",
            "hello",
            "--profile",
            "ops",
            "--chat-id",
            "@alerts",
            "--parse-mode",
            "html",
            "--silent",
            "--no-preview",
            "--protect",
            "--topic",
            "42",
            "--timeout",
            "3",
            "--max-chunks",
            "2",
            "--output",
            "json",
        ]))
        .expect("parse");
        let Command::Send(send) = command else {
            panic!("send")
        };
        assert_eq!(send.profile.as_deref(), Some("ops"));
        assert_eq!(send.topic, Some(42));
        assert_eq!(send.output, OutputMode::Json);
        assert!(send.silent && send.no_preview && send.protect);
    }

    #[test]
    fn validates_numeric_limits() {
        for values in [["--timeout", "0"], ["--max-chunks", "0"]] {
            assert!(parse(args(&values)).is_err());
        }
    }

    #[test]
    fn parses_config_set() {
        let command = parse(args(&[
            "config",
            "set",
            "ops",
            "--chat-id",
            "-1001",
            "--token-stdin",
        ]))
        .expect("parse");
        assert_eq!(
            command,
            Command::Config(ConfigArgs {
                config: None,
                action: ConfigAction::Set {
                    profile: "ops".to_owned(),
                    chat_id: Some("-1001".to_owned()),
                    token_stdin: true,
                },
            })
        );
    }
}
