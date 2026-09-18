//! Standalone onboarding and read-only diagnostics using the Telegram Bot API.

use std::fmt::Write as _;

use serde::Serialize;

use crate::Environment;
use crate::app::{Console, write_line};
use crate::bot::{BotChat, BotIdentityLookup};
use crate::cli::{OutputMode, SendArgs, SetupArgs, StatusArgs};
use crate::config::{Config, Profile, resolve_config_path};
use crate::error::TgsError;
use crate::output::SuccessReport;
use crate::telegram::{SendOptions, TelegramClient};

/// Runs interactive setup. Nothing is saved until the destination is confirmed.
/// A test message is sent only after explicit consent and a successful save.
pub fn execute(
    args: &SetupArgs,
    environment: &Environment,
    io: &mut impl Console,
    api_base: &str,
) -> Result<(), TgsError> {
    if !io.is_interactive() {
        return Err(TgsError::usage(
            "run `tgs setup` in a terminal; for automation use `tgs bot add PROFILE --token-stdin --chat-id ID`",
        ));
    }
    if args.chat_id.is_some() && (args.group || args.qr) {
        return Err(TgsError::usage(
            "--chat-id cannot be combined with --group or --qr",
        ));
    }
    let path = resolve_config_path(args.config.as_deref(), environment)?;
    let initial = Config::load_optional(&path)?;
    let requested = environment.requested_profile(args.profile.as_deref())?;
    let name = initial
        .selected_name(requested)?
        .unwrap_or("default")
        .to_owned();
    let destination_override = if args.group || args.qr {
        None
    } else {
        match args.chat_id.as_deref() {
            Some(value) => Some(value),
            None => environment.get("TGS_CHAT_ID")?,
        }
    };
    if let Some(chat) = destination_override {
        crate::config::validate_chat_id(chat)?;
    }
    let existing = initial.profiles.get(&name);
    write_line(
        io,
        &format!("Set up Telegram notifications for profile `{name}`."),
    )?;

    let token = if let Some(token) = environment.get("TGS_TOKEN")? {
        crate::config::validate_token(token)?;
        if !confirm(
            io,
            "Use TGS_TOKEN and save it in the local configuration? [y/N] ",
            false,
        )? {
            return canceled();
        }
        token.to_owned()
    } else if let Some(profile) = existing.filter(|profile| !profile.token.is_empty()) {
        if confirm(io, "Reuse this profile's saved bot token? [Y/n] ", true)? {
            profile.token.clone()
        } else {
            new_token(io)?
        }
    } else {
        new_token(io)?
    };
    crate::config::validate_token(&token)?;
    let client = TelegramClient::new(api_base, 10)?;
    let identity = client.get_me(&token)?;
    write_line(
        io,
        &format!(
            "Bot: {}",
            crate::telegram::redact(&identity.display_name, &token)
        ),
    )?;

    let chat = if let Some(id) = destination_override {
        client.inspect_chat(&token, id)?
    } else if let Some(id) = existing
        .and_then(|p| p.chat_id.as_deref())
        .filter(|_| !args.group && !args.qr)
    {
        if confirm(io, "Keep the saved destination? [Y/n] ", true)? {
            client.inspect_chat(&token, id)?
        } else {
            choose_destination(&client, &token, identity.username.as_deref(), args, io)?
        }
    } else {
        choose_destination(&client, &token, identity.username.as_deref(), args, io)?
    };

    write_line(
        io,
        &format!("Destination: {} [{}], ID {}", chat.name, chat.kind, chat.id),
    )?;
    if !confirm(io, "Save this bot and destination? [y/N] ", false)? {
        return canceled();
    }
    let select_default = initial
        .default_profile
        .as_deref()
        .is_none_or(|value| value == name)
        || confirm(io, "Use this profile by default? [Y/n] ", true)?;

    let mut config = initial.clone();
    config.profiles.insert(
        name.clone(),
        Profile {
            token: token.clone(),
            chat_id: Some(chat.id.to_string()),
        },
    );
    if select_default {
        config.default_profile = Some(name.clone());
    }
    config.save_if_unchanged(&path, &initial)?;
    let mut send_command = "tgs".to_owned();
    if args.config.is_some() {
        send_command.push_str(" --config PATH");
        write_line(
            io,
            "Replace PATH below with the same configuration path you supplied.",
        )?;
    }
    if !select_default || environment.contains("TGS_PROFILE") {
        send_command.push_str(&format!(" --profile {name}"));
    }
    if environment.contains("TGS_PROFILE") {
        write_line(
            io,
            "TGS_PROFILE is still active; the command below explicitly selects this profile.",
        )?;
    }
    if environment.contains("TGS_CHAT_ID") {
        write_line(
            io,
            "TGS_CHAT_ID is still active and overrides saved destinations; the command below pins the confirmed chat. Remove that environment setting to use saved defaults.",
        )?;
        send_command.push_str(&format!(" --chat-id {}", chat.id));
    }
    if environment.contains("TGS_TOKEN") {
        write_line(
            io,
            "TGS_TOKEN is still active and overrides saved bot tokens, including for other profiles.",
        )?;
    }
    send_command.push_str(" \"Hello\"");
    write_line(
        io,
        &format!("Saved profile `{name}`. Send with: {send_command}"),
    )?;
    if confirm(io, "Send a test notification now? [y/N] ", false)? {
        let options = SendOptions {
            parse_mode: crate::split::ParseMode::Plain,
            disable_notification: false,
            disable_web_page_preview: true,
            protect_content: false,
            message_thread_id: None,
        };
        let ids = client.send_chunks(
            &token,
            &chat.id.to_string(),
            &["tgs is ready to send notifications.".to_owned()],
            &options,
        )?;
        SuccessReport::new(&name, &chat.id.to_string(), ids).write_to(io, OutputMode::Text)?;
    }
    Ok(())
}

fn canceled<T>() -> Result<T, TgsError> {
    Err(TgsError::usage("setup canceled; configuration unchanged"))
}

fn new_token(io: &mut impl Console) -> Result<String, TgsError> {
    write_line(
        io,
        "If you need a bot, open https://t.me/BotFather and send /newbot.\nFollow its prompts, then paste the issued token below. An existing bot token also works.",
    )?;
    io.read_token(false)
}

fn confirm(io: &mut impl Console, prompt: &str, default: bool) -> Result<bool, TgsError> {
    loop {
        match io.prompt(prompt)?.trim().to_ascii_lowercase().as_str() {
            "" => return Ok(default),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            "q" | "quit" | "cancel" => return Err(TgsError::usage("setup canceled")),
            _ => write_line(io, "Please answer yes or no (or q to cancel).")?,
        }
    }
}

fn choose_destination(
    client: &TelegramClient,
    token: &str,
    username: Option<&str>,
    args: &SetupArgs,
    io: &mut impl Console,
) -> Result<BotChat, TgsError> {
    // Explicit pairing flags remain shortcuts, including --qr for private chat.
    if args.group || args.qr {
        return pair(client, token, username, args.group, args.qr, io);
    }
    write_line(
        io,
        "Where should notifications go?\n  1. My private chat with the bot (default)\n  2. A group\n  3. A channel",
    )?;
    loop {
        match io
            .prompt("Choose 1-3 [1] (q to cancel): ")?
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "" | "1" | "private" => return pair(client, token, username, false, false, io),
            "2" | "group" => return pair(client, token, username, true, false, io),
            "3" | "channel" => {
                write_line(
                    io,
                    "Make this bot a channel administrator with permission to post messages.",
                )?;
                loop {
                    let answer = io.prompt("Channel @username or numeric ID (q to cancel): ")?;
                    let id = answer.trim();
                    if matches!(id.to_ascii_lowercase().as_str(), "q" | "quit" | "cancel") {
                        return canceled();
                    }
                    if crate::config::validate_chat_id(id).is_err() {
                        write_line(
                            io,
                            "Enter a channel @username or a nonzero signed 64-bit ID.",
                        )?;
                        continue;
                    }
                    let chat = client.inspect_chat(token, id)?;
                    if chat.kind != "channel" {
                        return Err(TgsError::usage(
                            "that destination is not a channel; restart setup and choose private chat or group",
                        ));
                    }
                    return Ok(chat);
                }
            }
            "q" | "quit" | "cancel" => return canceled(),
            _ => write_line(
                io,
                "Choose 1 for private chat, 2 for group, or 3 for channel.",
            )?,
        }
    }
}

fn pair(
    client: &TelegramClient,
    token: &str,
    username: Option<&str>,
    group: bool,
    qr: bool,
    io: &mut impl Console,
) -> Result<BotChat, TgsError> {
    client.check_pairing(token)?;
    let username = username.ok_or_else(|| {
        TgsError::telegram("bot has no username for pairing; use `tgs setup --chat-id ID`")
    })?;
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random)
        .map_err(|_| TgsError::config("cannot generate a secure pairing code"))?;
    let challenge = encode_pairing_challenge(&random)?;
    let parameter = if group { "startgroup" } else { "start" };
    let link = format!("https://t.me/{username}?{parameter}={challenge}");
    write_line(
        io,
        if group {
            "Open this link and select the group for notifications:"
        } else {
            "Open this link in Telegram and press Start:"
        },
    )?;
    write_line(io, &link)?;
    if qr {
        let qr = qrcode::QrCode::new(link.as_bytes())
            .map_err(|_| TgsError::config("cannot render the pairing QR code"))?;
        write_line(
            io,
            &qr.render::<qrcode::render::unicode::Dense1x2>()
                .quiet_zone(true)
                .dark_color(qrcode::render::unicode::Dense1x2::Light)
                .light_color(qrcode::render::unicode::Dense1x2::Dark)
                .build(),
        )?;
    }
    loop {
        let answer = io.prompt("Press Enter after starting the bot in Telegram (q to cancel): ")?;
        if !answer.trim().is_empty() {
            if matches!(answer.trim(), "q" | "quit" | "cancel") {
                return canceled();
            }
            write_line(io, "Press Enter to check Telegram, or q to cancel.")?;
            continue;
        }
        if let Some(chat) = client.find_pairing(token, &challenge, username, group)? {
            return Ok(chat);
        }
        write_line(
            io,
            "The pairing message has not arrived. Use the link above and press Start again. If another application reads this bot's updates, use setup --chat-id ID.",
        )?;
    }
}

fn encode_pairing_challenge(random: &[u8; 16]) -> Result<String, TgsError> {
    let mut challenge = String::with_capacity(32);
    for byte in random {
        write!(&mut challenge, "{byte:02x}")
            .map_err(|_| TgsError::config("cannot encode pairing code"))?;
    }
    Ok(challenge)
}

/// Checks the resolved token and destination without sending or changing Telegram state.
/// A successful result proves API access, not that every send option is permitted.
pub fn status(
    args: &StatusArgs,
    environment: &Environment,
    io: &mut impl Console,
    api_base: &str,
) -> Result<(), TgsError> {
    let send = SendArgs {
        profile: args.profile.clone(),
        config: args.config.clone(),
        ..SendArgs::default()
    };
    let credentials = crate::resolve_credentials(&send, environment)?;
    let client = TelegramClient::new(api_base, 10)?;
    let identity = client.get_me(&credentials.token)?;
    let chat = client.inspect_chat(&credentials.token, &credentials.destination)?;
    let webhook = client.has_webhook(&credentials.token)?;
    #[derive(Serialize)]
    struct StatusReport<'a> {
        schema_version: u8,
        status: &'static str,
        profile: &'a str,
        bot_id: i64,
        bot_name: String,
        destination: &'a str,
        destination_name: &'a str,
        webhook_active: bool,
        delivery_tested: bool,
        sources: &'a crate::CredentialSources,
    }
    let report = StatusReport {
        schema_version: 1,
        status: "ok",
        profile: &credentials.profile,
        bot_id: identity.id,
        bot_name: crate::telegram::redact(&identity.display_name, &credentials.token),
        destination: &credentials.destination,
        destination_name: &chat.name,
        webhook_active: webhook,
        delivery_tested: false,
        sources: &credentials.sources,
    };
    match args.output {
        OutputMode::Json => write_line(
            io,
            &serde_json::to_string(&report)
                .map_err(|_| TgsError::config("cannot encode status report"))?,
        ),
        _ => write_line(
            io,
            &format!(
                "Profile: {} [{}]\nBot: {}\nDestination: {} ({}) [{}]\nToken: valid [{}]\nConfiguration source: {}\nDestination: accessible\nWebhook: {}\nNo message sent; delivery permissions have not been tested.",
                report.profile,
                report.sources.profile,
                report.bot_name,
                report.destination_name,
                report.destination,
                report.sources.destination,
                report.sources.token,
                report.sources.config,
                if webhook {
                    "active (sending still works)"
                } else {
                    "none"
                }
            ),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::encode_pairing_challenge;

    #[test]
    fn pairing_challenge_encoding_preserves_all_bytes_and_leading_zeroes() {
        let bytes = [
            0x00, 0x01, 0x0f, 0x10, 0x1f, 0x20, 0x7f, 0x80, 0x8f, 0x90, 0x9f, 0xa0, 0xef, 0xf0,
            0xfe, 0xff,
        ];
        assert_eq!(
            encode_pairing_challenge(&bytes).expect("encode challenge"),
            "00010f101f207f808f909fa0eff0feff"
        );
        assert_eq!(
            encode_pairing_challenge(&[0; 16]).expect("encode zero bytes"),
            "0".repeat(32)
        );
        assert_eq!(
            encode_pairing_challenge(&[0xff; 16]).expect("encode maximum bytes"),
            "f".repeat(32)
        );
    }
}
