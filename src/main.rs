#![forbid(unsafe_code)]

use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::process::ExitCode;

use tgs_cli::Environment;
use tgs_cli::cli::{self, Command, ConfigAction, ConfigArgs, MessageSource, OutputMode, SendArgs};
use tgs_cli::config::{Config, Profile, resolve_config_path, validate_profile_name};
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
        Command::Send(arguments) => execute_send(arguments),
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
    let mut config = Config::load_optional(&path)?;
    match arguments.action {
        ConfigAction::Init { profile } => {
            validate_profile_name(&profile)?;
            if config.profiles.contains_key(&profile) {
                return Err(TgsError::usage(format!(
                    "profile `{profile}` already exists"
                )));
            }
            config.profiles.insert(profile.clone(), Profile::default());
            if config.default_profile.is_none() {
                config.default_profile = Some(profile.clone());
            }
            config.save(&path)?;
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
            validate_profile_name(&profile)?;
            let should_read_token = token_stdin || chat_id.is_none();
            let token = if should_read_token {
                Some(read_token(token_stdin)?)
            } else {
                None
            };
            let entry = config.profiles.entry(profile.clone()).or_default();
            if let Some(token) = token {
                entry.token = token;
            }
            if let Some(chat_id) = chat_id {
                if chat_id.is_empty() {
                    return Err(TgsError::usage("chat ID must not be empty"));
                }
                entry.chat_id = Some(chat_id);
            }
            if config.default_profile.is_none() {
                config.default_profile = Some(profile.clone());
            }
            config.save(&path)?;
            Ok(format!("updated profile `{profile}`"))
        }
        ConfigAction::List => {
            let mut lines = Vec::new();
            for name in config.profiles.keys() {
                let suffix = if config.default_profile.as_deref() == Some(name) {
                    " (default)"
                } else {
                    ""
                };
                lines.push(format!("{name}{suffix}"));
            }
            Ok(lines.join("\n"))
        }
        ConfigAction::Show { profile } => {
            let name = profile
                .as_deref()
                .or(config.default_profile.as_deref())
                .ok_or_else(|| TgsError::config("no profile selected"))?;
            let item = config
                .profiles
                .get(name)
                .ok_or_else(|| TgsError::config(format!("profile `{name}` does not exist")))?;
            let destination = item.chat_id.as_deref().unwrap_or("<not set>");
            let token_state = if item.token.is_empty() {
                "not set"
            } else {
                "set"
            };
            Ok(format!(
                "profile: {name}\ndefault: {}\nchat_id: {destination}\ntoken: {token_state}",
                config.default_profile.as_deref() == Some(name)
            ))
        }
        ConfigAction::Use { profile } => {
            validate_profile_name(&profile)?;
            if !config.profiles.contains_key(&profile) {
                return Err(TgsError::config(format!(
                    "profile `{profile}` does not exist"
                )));
            }
            config.default_profile = Some(profile.clone());
            config.save(&path)?;
            Ok(format!("default profile is now `{profile}`"))
        }
        ConfigAction::Remove { profile } => {
            validate_profile_name(&profile)?;
            if config.profiles.remove(&profile).is_none() {
                return Err(TgsError::config(format!(
                    "profile `{profile}` does not exist"
                )));
            }
            if config.default_profile.as_deref() == Some(&profile) {
                config.default_profile = config.profiles.keys().next().cloned();
            }
            config.save(&path)?;
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
