use std::path::PathBuf;

use crate::error::TgsError;
use crate::split::ParseMode;

/// Complete command-line help text.
pub const HELP: &str = concat!(
    "tgs ",
    env!("CARGO_PKG_VERSION"),
    r#" — send Telegram text notifications

USAGE:
    tgs [send] [OPTIONS] [MESSAGE...]
    tgs config <COMMAND> [OPTIONS]
    tgs bot <COMMAND> [OPTIONS]

MESSAGE SOURCES (choose exactly one):
    MESSAGE...                 Positional words, joined with spaces
    -m, --message TEXT         Message text
    -f, --file PATH            Read message from a file
                               With no explicit source, piped stdin is read

SEND OPTIONS:
    --profile NAME             Select a named profile
    --chat-id ID               Override the destination only
    --config PATH              Override the configuration file
    --source LABEL             Message source label [env: TGS_SOURCE; default: tgs]
    --device NAME              Device label [env: TGS_DEVICE; default: hostname]
    --no-label                 Send the original message without a source label
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

BOT COMMANDS:
    create PROFILE [OPTIONS]   Create a bot with BotFather, then save it locally
    add PROFILE [OPTIONS]      Validate and add or replace a local bot profile
    list                       List local bot profiles
    info [PROFILE]             Show redacted local state and live bot identity
    chats [PROFILE]            List recent chats for destination setup
    use PROFILE                Set the default bot profile
    remove PROFILE             Remove only the local bot profile

BOT CREATE/ADD OPTIONS:
    --chat-id ID               Set or replace the default destination
    --token-stdin              Read the token from stdin (add only)
    --config PATH              Override the configuration file
"#,
);

/// Controls successful send output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputMode {
    /// Print nothing on success.
    Silent,
    /// Print a human-readable delivery summary.
    Text,
    /// Print the versioned JSON success report.
    Json,
}

/// An explicit message input selected by the user.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MessageSource {
    /// Positional words joined with spaces.
    Positional(String),
    /// Text supplied with `--message`.
    Message(String),
    /// A path supplied with `--file`.
    File(PathBuf),
}

/// Parsed arguments for sending a message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SendArgs {
    /// Explicit message source, or none for piped stdin/help behavior.
    pub source: Option<MessageSource>,
    /// Explicit profile override.
    pub profile: Option<String>,
    /// Explicit destination override.
    pub chat_id: Option<String>,
    /// Explicit configuration path.
    pub config: Option<PathBuf>,
    /// Explicit logical source for the generated message label.
    pub label_source: Option<String>,
    /// Explicit device name for the generated message label.
    pub device: Option<String>,
    /// Whether automatic message labeling is disabled.
    pub no_label: bool,
    /// Telegram parse mode.
    pub parse_mode: ParseMode,
    /// Whether Telegram notifications are disabled.
    pub silent: bool,
    /// Whether link previews are disabled.
    pub no_preview: bool,
    /// Whether forwarding and saving are restricted.
    pub protect: bool,
    /// Optional forum topic identifier.
    pub topic: Option<i64>,
    /// Global request timeout in seconds.
    pub timeout: u64,
    /// Maximum accepted message chunks.
    pub max_chunks: usize,
    /// Successful output mode.
    pub output: OutputMode,
}

impl Default for SendArgs {
    fn default() -> Self {
        Self {
            source: None,
            profile: None,
            chat_id: None,
            config: None,
            label_source: None,
            device: None,
            no_label: false,
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

/// Parsed configuration command and path override.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigArgs {
    /// Explicit configuration path.
    pub config: Option<PathBuf>,
    /// Requested configuration operation.
    pub action: ConfigAction,
}

/// Supported legacy configuration operations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigAction {
    /// Creates an empty profile and configuration when needed.
    Init {
        /// Profile to create.
        profile: String,
    },
    /// Sets a profile token and/or destination.
    Set {
        /// Profile to update.
        profile: String,
        /// Optional replacement destination.
        chat_id: Option<String>,
        /// Whether token input comes from stdin.
        token_stdin: bool,
    },
    /// Lists configured profiles.
    List,
    /// Shows redacted profile metadata.
    Show {
        /// Explicit profile, or none to resolve the default.
        profile: Option<String>,
    },
    /// Selects the default profile.
    Use {
        /// Existing profile to select.
        profile: String,
    },
    /// Removes a profile.
    Remove {
        /// Existing profile to remove.
        profile: String,
    },
    /// Prints the resolved configuration path.
    Path,
}

/// Parsed friendly bot command and path override.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BotArgs {
    /// Explicit configuration path.
    pub config: Option<PathBuf>,
    /// Requested bot-profile operation.
    pub action: BotAction,
}

/// Supported friendly bot-profile operations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BotAction {
    /// Guides BotFather creation and saves a new local profile.
    Create {
        /// New local profile name.
        profile: String,
        /// Optional default destination.
        chat_id: Option<String>,
    },
    /// Adds or replaces a token after live validation.
    Add {
        /// Local profile name.
        profile: String,
        /// Optional replacement destination.
        chat_id: Option<String>,
        /// Whether token input comes from stdin.
        token_stdin: bool,
    },
    /// Lists local bot profiles.
    List,
    /// Shows redacted local state and live identity.
    Info {
        /// Explicit profile, or none to resolve the default.
        profile: Option<String>,
    },
    /// Lists recently observed Telegram chats for destination setup.
    Chats {
        /// Explicit profile, or none to resolve the default.
        profile: Option<String>,
    },
    /// Selects the default bot profile.
    Use {
        /// Existing profile to select.
        profile: String,
    },
    /// Removes only the local profile.
    Remove {
        /// Existing profile to remove.
        profile: String,
    },
}

/// Top-level parsed command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    /// Sends a message.
    Send(SendArgs),
    /// Runs a legacy configuration operation.
    Config(ConfigArgs),
    /// Runs a friendly bot-profile operation.
    Bot(BotArgs),
    /// Prints help.
    Help,
    /// Prints the version.
    Version,
}

/// Parses process arguments excluding the executable name.
pub fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Command, TgsError> {
    let mut values: Vec<String> = arguments.into_iter().collect();
    if contains_standalone_flag(&values, &["-h", "--help"]) {
        return Ok(Command::Help);
    }
    if contains_standalone_flag(&values, &["-V", "--version"]) {
        return Ok(Command::Version);
    }
    if values.first().map(String::as_str) == Some("config") {
        values.remove(0);
        return parse_config(&values).map(Command::Config);
    }
    if values.first().map(String::as_str) == Some("bot") {
        values.remove(0);
        return parse_bot(&values).map(Command::Bot);
    }
    if values.first().map(String::as_str) == Some("send") {
        values.remove(0);
    }
    parse_send(&values).map(Command::Send)
}

fn contains_standalone_flag(values: &[String], flags: &[&str]) -> bool {
    const OPTIONS_WITH_VALUES: [&str; 14] = [
        "-m",
        "--message",
        "-f",
        "--file",
        "--profile",
        "--chat-id",
        "--config",
        "--source",
        "--device",
        "--parse-mode",
        "--topic",
        "--timeout",
        "--max-chunks",
        "--output",
    ];
    let mut index = 0;
    while index < values.len() {
        let value = values[index].as_str();
        if value == "--" {
            return false;
        }
        if flags.contains(&value) {
            return true;
        }
        index += if OPTIONS_WITH_VALUES.contains(&value) {
            2
        } else {
            1
        };
    }
    false
}

fn parse_bot(values: &[String]) -> Result<BotArgs, TgsError> {
    let action_name = values
        .first()
        .ok_or_else(|| TgsError::usage("missing bot command; use `tgs --help`"))?;
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
                "bot {command} requires exactly one profile name"
            )));
        }
        crate::config::validate_profile_name(&positional[0])?;
        Ok(positional[0].clone())
    };
    let no_input_flags = |command: &str| -> Result<(), TgsError> {
        if chat_id.is_some() || token_stdin {
            Err(TgsError::usage(format!(
                "--chat-id and --token-stdin are not valid with `bot {command}`"
            )))
        } else {
            Ok(())
        }
    };

    if let Some(value) = chat_id.as_deref() {
        crate::config::validate_chat_id(value)?;
    }

    let action = match action_name.as_str() {
        "create" => {
            if token_stdin {
                return Err(TgsError::usage(
                    "--token-stdin is not valid with `bot create`",
                ));
            }
            BotAction::Create {
                profile: one_profile("create")?,
                chat_id,
            }
        }
        "add" => BotAction::Add {
            profile: one_profile("add")?,
            chat_id,
            token_stdin,
        },
        "list" => {
            no_input_flags("list")?;
            ensure_no_bot_positionals(&positional, "list")?;
            BotAction::List
        }
        "info" | "chats" => {
            no_input_flags(action_name)?;
            if positional.len() > 1 {
                return Err(TgsError::usage(format!(
                    "bot {action_name} accepts at most one profile"
                )));
            }
            if let Some(profile) = positional.first() {
                crate::config::validate_profile_name(profile)?;
            }
            if action_name == "info" {
                BotAction::Info {
                    profile: positional.first().cloned(),
                }
            } else {
                BotAction::Chats {
                    profile: positional.first().cloned(),
                }
            }
        }
        "use" => {
            no_input_flags("use")?;
            BotAction::Use {
                profile: one_profile("use")?,
            }
        }
        "remove" => {
            no_input_flags("remove")?;
            BotAction::Remove {
                profile: one_profile("remove")?,
            }
        }
        unknown => return Err(TgsError::usage(format!("unknown bot command `{unknown}`"))),
    };
    validate_config_path(config.as_deref())?;
    Ok(BotArgs { config, action })
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
            "--source" => result.label_source = Some(next_value(values, &mut index, value)?),
            "--device" => result.device = Some(next_value(values, &mut index, value)?),
            "--no-label" => result.no_label = true,
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
    if result.no_label && (result.label_source.is_some() || result.device.is_some()) {
        return Err(TgsError::usage(
            "--no-label cannot be combined with --source or --device",
        ));
    }
    if let Some(source) = result.label_source.as_deref() {
        crate::validate_identity(source, 64, "source")?;
    }
    if let Some(device) = result.device.as_deref() {
        crate::validate_identity(device, 255, "device")?;
    }
    if let Some(profile) = result.profile.as_deref() {
        crate::config::validate_profile_name(profile)?;
    }
    if let Some(chat_id) = result.chat_id.as_deref() {
        crate::config::validate_chat_id(chat_id)?;
    }
    validate_config_path(result.config.as_deref())?;
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
        crate::config::validate_profile_name(&positional[0])?;
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

    if let Some(value) = chat_id.as_deref() {
        crate::config::validate_chat_id(value)?;
    }

    let action = match action_name.as_str() {
        "init" => {
            no_set_flags()?;
            if positional.len() > 1 {
                return Err(TgsError::usage("config init accepts at most one profile"));
            }
            let profile = positional
                .first()
                .cloned()
                .unwrap_or_else(|| "default".to_owned());
            crate::config::validate_profile_name(&profile)?;
            ConfigAction::Init { profile }
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
            if let Some(profile) = positional.first() {
                crate::config::validate_profile_name(profile)?;
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
    validate_config_path(config.as_deref())?;
    Ok(ConfigArgs { config, action })
}

fn validate_config_path(path: Option<&std::path::Path>) -> Result<(), TgsError> {
    if path.is_some_and(|value| value.as_os_str().is_empty()) {
        Err(TgsError::usage("--config must not be empty"))
    } else {
        Ok(())
    }
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

fn ensure_no_bot_positionals(values: &[String], command: &str) -> Result<(), TgsError> {
    if values.is_empty() {
        Ok(())
    } else {
        Err(TgsError::usage(format!(
            "bot {command} does not accept arguments"
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
    fn help_and_version_are_available_at_nested_command_positions() {
        for input in [
            vec!["--help"],
            vec!["send", "--profile", "ops", "--help"],
            vec!["config", "set", "ops", "--help"],
            vec!["bot", "create", "ops", "--help"],
        ] {
            assert_eq!(parse(args(&input)).expect("help"), Command::Help);
        }
        assert_eq!(
            parse(args(&["send", "--version"])).expect("version"),
            Command::Version
        );
    }

    #[test]
    fn help_like_option_values_and_positional_text_are_preserved() {
        let Command::Send(message) = parse(args(&["--message", "--help"])).expect("message value")
        else {
            panic!("expected send command")
        };
        assert_eq!(
            message.source,
            Some(MessageSource::Message("--help".to_owned()))
        );

        let Command::Send(positional) =
            parse(args(&["--", "--version"])).expect("positional value")
        else {
            panic!("expected send command")
        };
        assert_eq!(
            positional.source,
            Some(MessageSource::Positional("--version".to_owned()))
        );
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
            "--source",
            "codex",
            "--device",
            "workstation-01",
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
        assert_eq!(send.label_source.as_deref(), Some("codex"));
        assert_eq!(send.device.as_deref(), Some("workstation-01"));
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
    fn rejects_no_label_with_explicit_identity() {
        for values in [
            ["--no-label", "--source", "ci"],
            ["--no-label", "--device", "runner"],
        ] {
            assert!(parse(args(&values)).is_err());
        }
    }

    #[test]
    fn validates_explicit_identity_values_during_parsing() {
        assert!(parse(args(&["--source", &"é".repeat(64)])).is_ok());
        assert!(parse(args(&["--source", &"é".repeat(65)])).is_err());
        assert!(parse(args(&["--device", &"界".repeat(255)])).is_ok());
        assert!(parse(args(&["--device", &"界".repeat(256)])).is_err());
        for invalid in ["", " leading", "trailing ", "line\nbreak"] {
            assert!(parse(args(&["--source", invalid])).is_err());
            assert!(parse(args(&["--device", invalid])).is_err());
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

    #[test]
    fn parses_every_bot_command_and_supported_flag() {
        assert_eq!(
            parse(args(&[
                "bot",
                "create",
                "new",
                "--chat-id",
                "-1001",
                "--config",
                "custom.json",
            ]))
            .expect("create"),
            Command::Bot(BotArgs {
                config: Some(PathBuf::from("custom.json")),
                action: BotAction::Create {
                    profile: "new".to_owned(),
                    chat_id: Some("-1001".to_owned()),
                },
            })
        );
        assert_eq!(
            parse(args(&["bot", "add", "ops", "--token-stdin"])).expect("add"),
            Command::Bot(BotArgs {
                config: None,
                action: BotAction::Add {
                    profile: "ops".to_owned(),
                    chat_id: None,
                    token_stdin: true,
                },
            })
        );
        for (input, expected) in [
            (vec!["bot", "list"], BotAction::List),
            (vec!["bot", "info"], BotAction::Info { profile: None }),
            (
                vec!["bot", "info", "ops"],
                BotAction::Info {
                    profile: Some("ops".to_owned()),
                },
            ),
            (vec!["bot", "chats"], BotAction::Chats { profile: None }),
            (
                vec!["bot", "chats", "ops"],
                BotAction::Chats {
                    profile: Some("ops".to_owned()),
                },
            ),
            (
                vec!["bot", "use", "ops"],
                BotAction::Use {
                    profile: "ops".to_owned(),
                },
            ),
            (
                vec!["bot", "remove", "ops"],
                BotAction::Remove {
                    profile: "ops".to_owned(),
                },
            ),
        ] {
            let Command::Bot(parsed) = parse(args(&input)).expect("bot command") else {
                panic!("expected bot command")
            };
            assert_eq!(parsed.action, expected);
        }
    }

    #[test]
    fn rejects_missing_bot_arguments_and_invalid_flag_combinations() {
        for input in [
            vec!["bot"],
            vec!["bot", "create"],
            vec!["bot", "add"],
            vec!["bot", "use"],
            vec!["bot", "remove", "one", "two"],
            vec!["bot", "list", "extra"],
            vec!["bot", "info", "one", "two"],
            vec!["bot", "chats", "one", "two"],
            vec!["bot", "create", "ops", "--token-stdin"],
            vec!["bot", "list", "--chat-id", "1"],
            vec!["bot", "use", "ops", "--token-stdin"],
            vec!["bot", "unknown"],
        ] {
            assert!(parse(args(&input)).is_err(), "{input:?}");
        }
    }
}
