#![doc = "Process I/O and exit handling for the tgs executable."]
#![forbid(unsafe_code)]

use std::fs::File;
use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;

use tgs_cli::app::Console;
use tgs_cli::cli::{self, MessageSource};
use tgs_cli::error::{ErrorKind, TgsError};
use tgs_cli::{Environment, input};

fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let json_errors = !io::stderr().is_terminal() || cli::wants_json_errors(&arguments);
    let mut console = ProcessConsole;
    let result =
        tgs_cli::app::read_command(arguments, &mut console).and_then(|command| match command {
            Some(command) => tgs_cli::app::execute(command, &Environment::capture(), &mut console),
            None => Ok(()),
        });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ignored = error.write_to(&mut io::stderr().lock(), json_errors);
            ExitCode::from(error.exit_code())
        }
    }
}

struct ProcessConsole;

impl Write for ProcessConsole {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        io::stdout().lock().write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        io::stdout().lock().flush()
    }
}

impl Console for ProcessConsole {
    fn is_interactive(&self) -> bool {
        io::stdin().is_terminal() && io::stdout().is_terminal()
    }

    fn read_message(
        &mut self,
        source: Option<&MessageSource>,
        limit: usize,
    ) -> Result<Option<String>, TgsError> {
        match source {
            Some(MessageSource::Positional(message) | MessageSource::Message(message)) => {
                if message.len() > limit {
                    return Err(TgsError::usage("message exceeds the input size limit"));
                }
                Ok(Some(message.clone()))
            }
            Some(MessageSource::File(path)) => {
                let file = File::open(path).map_err(|_| {
                    TgsError::config("cannot open message file; check its path and permissions")
                })?;
                input::read_text(file, limit, ErrorKind::Config, "message file").map(Some)
            }
            None if io::stdin().is_terminal() => Ok(None),
            None => input::read_text(io::stdin().lock(), limit, ErrorKind::Usage, "stdin message")
                .map(Some),
        }
    }

    fn read_token(&mut self, from_stdin: bool) -> Result<String, TgsError> {
        let token = if from_stdin {
            input::read_text(
                io::stdin().lock(),
                input::MAX_TOKEN_BYTES,
                ErrorKind::Usage,
                "token input",
            )?
            .trim_end_matches(['\r', '\n'])
            .to_owned()
        } else {
            if !self.is_interactive() {
                return Err(TgsError::usage(
                    "hidden token input requires a terminal; use --token-stdin for automation",
                ));
            }
            rpassword::prompt_password("Telegram bot token (input hidden): ")
                .map_err(|_| TgsError::config("cannot read hidden token"))?
        };
        if token.len() > input::MAX_TOKEN_BYTES {
            return Err(TgsError::usage("token input exceeds the size limit"));
        }
        tgs_cli::config::validate_token(&token)?;
        Ok(token)
    }

    fn prompt(&mut self, message: &str) -> Result<String, TgsError> {
        self.prompt_with_limit(message, 1024)
    }

    fn prompt_with_limit(&mut self, message: &str, limit: usize) -> Result<String, TgsError> {
        if !self.is_interactive() {
            return Err(TgsError::usage("interactive input requires a terminal"));
        }
        tgs_cli::app::write_text(self, message)?;
        input::read_prompt_line(io::stdin().lock(), limit)
    }
}
