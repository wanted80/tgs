//! Command workflows with injectable process I/O for offline testing.

use std::io::Write;

use crate::Environment;
use crate::cli::{self, BotAction, BotArgs, Command, ConfigAction, ConfigArgs, MessageSource};
use crate::config::{self, resolve_config_path};
use crate::error::TgsError;

/// Process I/O used by command workflows. Implementations must not echo tokens.
pub trait Console: Write {
    /// Whether interactive prompts are available.
    fn is_interactive(&self) -> bool;
    /// Reads a message within the supplied byte budget, or returns none for terminal stdin.
    fn read_message(
        &mut self,
        source: Option<&MessageSource>,
        limit: usize,
    ) -> Result<Option<String>, TgsError>;
    /// Reads a token from a hidden prompt or bounded stdin.
    fn read_token(&mut self, from_stdin: bool) -> Result<String, TgsError>;
    /// Prompts for a bounded single line; EOF must be an error, not consent.
    fn prompt(&mut self, message: &str) -> Result<String, TgsError>;
}

/// Runs a command using the official Telegram endpoint.
pub fn execute(
    command: Command,
    environment: &Environment,
    io: &mut impl Console,
) -> Result<(), TgsError> {
    execute_with_base(command, environment, io, crate::telegram::API_BASE)
}

/// Runs the same command workflow against an injected endpoint for offline tests.
pub fn execute_with_base(
    command: Command,
    environment: &Environment,
    io: &mut impl Console,
    api_base: &str,
) -> Result<(), TgsError> {
    match command {
        Command::Help => write_text(io, cli::HELP),
        Command::Version => write_line(io, concat!("tgs ", env!("CARGO_PKG_VERSION"))),
        Command::Config(args) => {
            let output = execute_config(args, environment, io)?;
            if output.is_empty() {
                Ok(())
            } else {
                write_line(io, &output)
            }
        }
        Command::Bot(args) => {
            let output = execute_bot(args, environment, io, api_base)?;
            if output.is_empty() {
                Ok(())
            } else {
                write_line(io, &output)
            }
        }
        Command::Send(args) => {
            let Some(message) = io.read_message(
                args.source.as_ref(),
                crate::input::message_limit(args.max_chunks),
            )?
            else {
                return write_text(io, cli::HELP);
            };
            let report = crate::send_message_with_base(&args, &message, environment, api_base)?;
            report.write_to(io, args.output)
        }
        Command::Setup(args) => crate::setup::execute(&args, environment, io, api_base),
        Command::Status(args) => crate::setup::status(&args, environment, io, api_base),
    }
}

/// Writes and flushes output without panicking or exposing underlying error data.
pub fn write_text(writer: &mut impl Write, text: &str) -> Result<(), TgsError> {
    writer
        .write_all(text.as_bytes())
        .and_then(|()| writer.flush())
        .map_err(|_| TgsError::config("cannot write output; check the destination or pipe"))
}

/// Writes one line and checks the flush.
pub fn write_line(writer: &mut impl Write, text: &str) -> Result<(), TgsError> {
    write_text(writer, &format!("{text}\n"))
}

fn execute_bot(
    arguments: BotArgs,
    environment: &Environment,
    io: &mut impl Console,
    api_base: &str,
) -> Result<String, TgsError> {
    let client = crate::telegram::TelegramClient::new(api_base, 10)?;
    let has_config_override = arguments.config.is_some();
    let path = resolve_config_path(arguments.config.as_deref(), environment)?;
    match arguments.action {
        BotAction::Create { profile, chat_id } => {
            crate::bot::validate_create(&path, &profile, chat_id.as_deref())?;
            write_line(
                io,
                "Open https://t.me/BotFather, send /newbot, and follow the prompts.\nThen paste the issued token below; input is hidden.",
            )?;
            let token = io.read_token(false)?;
            let identity =
                crate::bot::add(&path, &profile, chat_id.as_deref(), &token, true, &client)?;
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
            let token = io.read_token(token_stdin)?;
            let identity =
                crate::bot::add(&path, &profile, chat_id.as_deref(), &token, false, &client)?;
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
        BotAction::List => Ok(crate::bot::list(&path)?
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
            let info = crate::bot::info(&path, profile.as_deref(), &client)?;
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
            let result = crate::bot::chats(&path, profile.as_deref(), &client)?;
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
                "set one with: tgs config set --chat-id ID -- {}{}",
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
            crate::bot::use_profile(&path, &profile)?;
            Ok(format!("default bot profile is now `{profile}`"))
        }
        BotAction::Remove { profile } => {
            crate::bot::remove(&path, &profile)?;
            Ok(format!(
                "removed local profile `{profile}`; the Telegram bot still exists"
            ))
        }
    }
}
fn execute_config(
    arguments: ConfigArgs,
    environment: &Environment,
    io: &mut impl Console,
) -> Result<String, TgsError> {
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
                Some(io.read_token(token_stdin)?)
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
