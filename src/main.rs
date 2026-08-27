#![doc = "Process I/O and exit handling for the `tgs` executable."]
#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::expect_used, clippy::panic))]

use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::process::ExitCode;

use tgs_cli::Environment;
use tgs_cli::cli::{
    self, BotAction, BotArgs, Command, ConfigAction, ConfigArgs, MessageSource, OutputMode,
    SendArgs,
};
use tgs_cli::config::{self, resolve_config_path};
use tgs_cli::error::{FailureReport, TgsError};

fn main() -> ExitCode {
    ExitCode::from(run())
}

fn run() -> u8 {
    let command = match cli::parse(std::env::args().skip(1)) {
        Ok(command) => command,
        Err(error) => return fail(&error),
    };
    match command {
        Command::Help => {
            print!("{}", cli::HELP);
            0
        }
        Command::Version => {
            println!("tgs {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Command::Config(arguments) => match execute_config(arguments, &Environment::capture()) {
            Ok(output) => {
                if !output.is_empty() {
                    println!("{output}");
                }
                0
            }
            Err(error) => fail(&error),
        },
        Command::Bot(arguments) => match execute_bot(arguments, &Environment::capture()) {
            Ok(output) => {
                if !output.is_empty() {
                    println!("{output}");
                }
                0
            }
            Err(error) => fail(&error),
        },
        Command::Send(arguments) => execute_send(arguments),
    }
}

fn execute_bot(arguments: BotArgs, environment: &Environment) -> Result<String, TgsError> {
    let has_config_override = arguments.config.is_some();
    let path = resolve_config_path(arguments.config.as_deref(), environment)?;
    match arguments.action {
        BotAction::Create { profile, chat_id } => {
            tgs_cli::bot::validate_create(&path, &profile, chat_id.as_deref())?;
            println!(
                "Open https://t.me/BotFather, send /newbot, and follow the prompts.\nThen paste the issued token below; input is hidden."
            );
            let token = read_token(false)?;
            let identity =
                tgs_cli::bot::add_live(&path, &profile, chat_id.as_deref(), &token, true)?;
            Ok(format!(
                "created bot profile `{profile}` for {}{}",
                identity.display_name,
                identity
                    .username
                    .as_deref()
                    .map(|value| format!(" (@{value})"))
                    .unwrap_or_default()
            ))
        }
        BotAction::Add {
            profile,
            chat_id,
            token_stdin,
        } => {
            let token = read_token(token_stdin)?;
            let identity =
                tgs_cli::bot::add_live(&path, &profile, chat_id.as_deref(), &token, false)?;
            Ok(format!(
                "saved bot profile `{profile}` for {}{}",
                identity.display_name,
                identity
                    .username
                    .as_deref()
                    .map(|value| format!(" (@{value})"))
                    .unwrap_or_default()
            ))
        }
        BotAction::List => Ok(tgs_cli::bot::list(&path)?
            .into_iter()
            .map(|(name, is_default)| {
                if is_default {
                    format!("{name} (default)")
                } else {
                    name
                }
            })
            .collect::<Vec<_>>()
            .join("\n")),
        BotAction::Info { profile } => {
            let info = tgs_cli::bot::info_live(&path, profile.as_deref())?;
            let username = info
                .identity
                .username
                .as_deref()
                .map(|value| format!("@{value}"))
                .unwrap_or_else(|| "<not set>".to_owned());
            Ok(format!(
                "profile: {}\ndefault: {}\nchat_id: {}\ntoken: set\nbot_id: {}\nname: {}\nusername: {}",
                info.profile,
                info.is_default,
                info.chat_id.as_deref().unwrap_or("<not set>"),
                info.identity.id,
                info.identity.display_name,
                username
            ))
        }
        BotAction::Chats { profile } => {
            let result = tgs_cli::bot::chats_live(&path, profile.as_deref())?;
            if result.chats.is_empty() {
                return Ok(format!(
                    "no recent chats found for `{}`; send the bot a message (or add it to the destination), then try again",
                    result.profile
                ));
            }
            let mut lines = vec![format!("recent chats for `{}`:", result.profile)];
            lines.extend(
                result
                    .chats
                    .into_iter()
                    .map(|chat| format!("{} [{}] {}", chat.id, chat.kind, chat.name)),
            );
            lines.push(format!(
                "set one with: tgs config set {} --chat-id ID{}",
                result.profile,
                if has_config_override {
                    " (reuse the same --config PATH)"
                } else {
                    ""
                }
            ));
            Ok(lines.join("\n"))
        }
        BotAction::Use { profile } => {
            tgs_cli::bot::use_profile(&path, &profile)?;
            Ok(format!("default bot profile is now `{profile}`"))
        }
        BotAction::Remove { profile } => {
            tgs_cli::bot::remove(&path, &profile)?;
            Ok(format!(
                "removed local profile `{profile}`; the Telegram bot still exists"
            ))
        }
    }
}

fn execute_send(arguments: SendArgs) -> u8 {
    let message = match read_message(arguments.source.as_ref()) {
        Ok(Some(message)) => message,
        Ok(None) => {
            print!("{}", cli::HELP);
            return 0;
        }
        Err(error) => return fail(&error),
    };
    match tgs_cli::send_message(&arguments, &message, &Environment::capture()) {
        Ok(report) => {
            match arguments.output {
                OutputMode::Silent => {}
                OutputMode::Text => println!("{}", report.text()),
                OutputMode::Json => match serde_json::to_string(&report) {
                    Ok(json) => println!("{json}"),
                    Err(error) => {
                        return fail(&TgsError::config(format!(
                            "cannot encode success output: {error}"
                        )));
                    }
                },
            }
            0
        }
        Err(error) => fail(&error),
    }
}

fn read_message(source: Option<&MessageSource>) -> Result<Option<String>, TgsError> {
    match source {
        Some(MessageSource::Positional(message) | MessageSource::Message(message)) => {
            Ok(Some(message.clone()))
        }
        Some(MessageSource::File(path)) => fs::read_to_string(path).map(Some).map_err(|error| {
            TgsError::config(format!(
                "cannot read message file `{}`: {error}",
                path.display()
            ))
        }),
        None if io::stdin().is_terminal() => Ok(None),
        None => {
            let mut message = String::new();
            io::stdin().read_to_string(&mut message).map_err(|error| {
                TgsError::usage(format!("cannot read message from stdin: {error}"))
            })?;
            Ok(Some(message))
        }
    }
}

fn execute_config(arguments: ConfigArgs, environment: &Environment) -> Result<String, TgsError> {
    let path = resolve_config_path(arguments.config.as_deref(), environment)?;
    if arguments.action == ConfigAction::Path {
        return Ok(path.display().to_string());
    }
    match arguments.action {
        ConfigAction::Init { profile } => {
            config::init_profile(&path, &profile)?;
            Ok(format!(
                "initialized profile `{profile}` in {}",
                path.display()
            ))
        }
        ConfigAction::Set {
            profile,
            chat_id,
            token_stdin,
        } => {
            let should_read_token = token_stdin || chat_id.is_none();
            let token = if should_read_token {
                Some(read_token(token_stdin)?)
            } else {
                None
            };
            config::set_profile(&path, &profile, chat_id.as_deref(), token.as_deref())?;
            Ok(format!("updated profile `{profile}`"))
        }
        ConfigAction::List => Ok(config::list_profiles(&path)?
            .into_iter()
            .map(|(name, is_default)| {
                if is_default {
                    format!("{name} (default)")
                } else {
                    name
                }
            })
            .collect::<Vec<_>>()
            .join("\n")),
        ConfigAction::Show { profile } => {
            let info = config::profile_info(&path, profile.as_deref())?;
            let destination = info.chat_id.as_deref().unwrap_or("<not set>");
            let token_state = if info.token_is_set { "set" } else { "not set" };
            Ok(format!(
                "profile: {}\ndefault: {}\nchat_id: {destination}\ntoken: {token_state}",
                info.name, info.is_default
            ))
        }
        ConfigAction::Use { profile } => {
            config::use_profile(&path, &profile)?;
            Ok(format!("default profile is now `{profile}`"))
        }
        ConfigAction::Remove { profile } => {
            config::remove_profile(&path, &profile)?;
            Ok(format!("removed profile `{profile}`"))
        }
        ConfigAction::Path => Ok(path.display().to_string()),
    }
}

fn read_token(from_stdin: bool) -> Result<String, TgsError> {
    let token = if from_stdin {
        let mut value = String::new();
        io::stdin()
            .read_to_string(&mut value)
            .map_err(|error| TgsError::usage(format!("cannot read token from stdin: {error}")))?;
        value.trim_end_matches(['\r', '\n']).to_owned()
    } else {
        rpassword::prompt_password("Telegram bot token: ")
            .map_err(|error| TgsError::config(format!("cannot read hidden token: {error}")))?
    };
    if token.is_empty() {
        return Err(TgsError::usage("token must not be empty"));
    }
    Ok(token)
}

fn fail(error: &TgsError) -> u8 {
    let report = FailureReport::from(error);
    let mut stderr = io::stderr().lock();
    match serde_json::to_writer(&mut stderr, &report) {
        Ok(()) => {
            let _ignored = stderr.write_all(b"\n");
        }
        Err(_) => {
            let _ignored = writeln!(stderr, "error: {}", error.message());
        }
    }
    error.exit_code()
}
