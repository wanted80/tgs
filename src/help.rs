//! Short entry-point help and focused references for existing commands.

macro_rules! page {
    ($title:literal, $body:literal) => {
        concat!(
            "tgs ",
            env!("CARGO_PKG_VERSION"),
            " — ",
            $title,
            "\n\n",
            $body
        )
    };
}

pub(crate) const OVERVIEW: &str = page!(
    "send Telegram text notifications",
    r#"GET STARTED:
    tgs setup                   Connect your bot and choose a destination
    tgs "Hello"                 Send a notification (quiet on success)
    tgs status                  Check the active bot, destination, and overrides

EVERYDAY USE:
    tgs --verbose "Done"         Show a delivery summary
    tgs --file notification.txt  Send a UTF-8 text file
    command | tgs               Send piped output
    tgs --no-label "Hello"       Omit the automatic source and hostname label

MORE HELP:
    tgs send --help              Message sources and all delivery options
    tgs setup --help             Existing bots, groups, channels, and QR pairing
    tgs status --help            Diagnostics and JSON output

ADVANCED PROFILE MANAGEMENT:
    tgs bot --help               Validate and manage saved bot profiles
    tgs config --help            Edit local configuration without Telegram

Use --version for the installed version. Bot creation still uses BotFather.
"#
);

const SEND: &str = page!(
    "send options",
    r#"USAGE:
    tgs [send] [OPTIONS] [MESSAGE...]

Choose one source: positional words, -m/--message TEXT, -f/--file PATH,
or piped stdin when no source is specified. Use -- before option-like text.

OPTIONS:
    --profile NAME              Profile [env: TGS_PROFILE; then saved default]
    --chat-id ID                Override destination [env: TGS_CHAT_ID]
    --config PATH              Configuration file [env: TGS_CONFIG]
    --source LABEL             Source label [env: TGS_SOURCE; default: tgs]
    --device NAME              Device label [env: TGS_DEVICE; default: hostname]
    --no-label                 Omit labels and ignore label environment settings
    --parse-mode MODE          plain, html, markdown-v2 [default: plain]
    --silent                   Disable Telegram notification sound
    --no-preview               Disable link previews
    --protect                  Restrict forwarding and saving
    --topic ID                 Positive forum topic ID
    --timeout SECONDS          Request timeout, 1-300 [default: 10]
    --max-chunks N             Plain-text chunk cap [default: 10]
    --output MODE              silent, text, json [default: silent]
    --verbose                  Alias for --output text

TGS_TOKEN overrides the saved token. Tokens are never command-line arguments.
Input must be UTF-8 text; tabs and line endings are allowed, other control
characters (including terminal color escapes) are rejected before sending.
Use tgs setup to connect a bot, or tgs status to inspect active settings.
"#
);

const SETUP: &str = page!(
    "guided setup",
    r#"USAGE:
    tgs setup [PROFILE] [OPTIONS]

Reuse an existing bot or get a token from BotFather. With no destination set,
choose your private bot chat, a group, or a channel in the wizard. Private chat
is the default. Save only after confirming the destination; a test is optional.

OPTIONS:
    --profile NAME              Alternative to the positional profile
    --config PATH              Configuration file [env: TGS_CONFIG]
    --chat-id ID                Known destination; skips interactive discovery
    --group                    Pair directly with a group
    --qr                       Show a private-chat QR link (group with --group)

--group and --qr request fresh pairing, ignoring saved/environment destinations.
Neither can be combined with --chat-id. Channels need a bot with posting rights.
Existing webhooks/filters are never changed; use --chat-id for a busy bot.
TGS_PROFILE selects a profile; TGS_TOKEN is saved only with your consent.
For option-like names: tgs setup -- -ops
For automation: tgs bot add --help
"#
);

const STATUS: &str = page!(
    "connection status",
    r#"USAGE:
    tgs status [PROFILE] [--profile NAME] [--config PATH] [--output text|json]

Check the effective send settings without sending a message or changing state.
Reports the active profile, bot, destination, webhook, and where settings came
from: explicit options, environment overrides, or saved configuration.
TGS_PROFILE, TGS_TOKEN, TGS_CHAT_ID, and TGS_CONFIG follow send precedence.
Use either a positional profile or --profile, not both.

Status does not prove posting permissions. Use the optional test in tgs setup
to verify delivery. For option-like names: tgs status -- -ops
"#
);

const BOT: &str = page!(
    "advanced bot profiles",
    r#"USAGE:
    tgs bot <COMMAND> [OPTIONS]

For normal onboarding, use tgs setup instead.
    create PROFILE              BotFather instructions and token validation
    add PROFILE                 Validate and add/replace a saved bot token
    list                        List saved profiles
    info [PROFILE]              Inspect a saved token's live identity
    chats [PROFILE]             Discover chats using a saved token
    use PROFILE                 Select the default profile
    remove PROFILE              Remove only a local profile

All commands accept --config PATH. Run tgs bot COMMAND --help for details.
Omitted info/chats profiles use TGS_PROFILE, then the default or only profile.
These commands use saved tokens, not TGS_TOKEN; tgs status checks effective sends.
Put options before -- for option-like names: tgs bot remove -- -ops
"#
);

const CONFIG: &str = page!(
    "advanced local configuration",
    r#"USAGE:
    tgs config <COMMAND> [OPTIONS]

These commands do not contact Telegram. For normal onboarding, use tgs setup.
    init [PROFILE]              Create an empty profile (default name: default)
    set PROFILE                 Update a token and/or destination
    list                        List saved profiles
    show [PROFILE]              Show saved metadata, never a token value
    use PROFILE                 Select the default profile
    remove PROFILE              Remove only a local profile
    path                        Print the configuration file path

All commands accept --config PATH. Run tgs config COMMAND --help for details.
Omitted show profiles use TGS_PROFILE, then the default or only profile.
Use tgs status for effective send settings, including credential overrides.
Put options before -- for option-like names: tgs config show -- -ops
"#
);

pub(crate) fn for_arguments(values: &[String]) -> Option<&'static str> {
    let command = values.first().map(String::as_str)?;
    let action = values.get(1).map(String::as_str).unwrap_or("");
    Some(match (command, action) {
        ("-h" | "--help", _) => return None,
        ("setup", _) => SETUP,
        ("status", _) => STATUS,
        ("bot", "create") => page!(
            "create a saved bot profile",
            "USAGE:\n    tgs bot create PROFILE [--chat-id ID] [--config PATH]\n\nGuides BotFather creation, reads a hidden token, validates it, and saves a new\nprofile. Existing profiles are not replaced. Use tgs setup for chat pairing.\nUse -- before option-like profile names.\n"
        ),
        ("bot", "add") => page!(
            "add or update a saved bot",
            "USAGE:\n    tgs bot add PROFILE [--chat-id ID] [--token-stdin] [--config PATH]\n\nReads a hidden token (or stdin), validates it with Telegram, and saves it.\nAn omitted destination preserves the existing one. Tokens are never arguments.\nUse -- before option-like profile names.\n"
        ),
        ("bot", "info") => page!(
            "saved bot identity",
            "USAGE:\n    tgs bot info [PROFILE] [--config PATH]\n\nSelects PROFILE, TGS_PROFILE, the default, or the only saved profile.\nValidates its saved token, ignoring TGS_TOKEN. Never prints token values.\nUse tgs status for the effective send connection, or -- before option-like names.\n"
        ),
        ("bot", "chats") => page!(
            "discover recent chats",
            "USAGE:\n    tgs bot chats [PROFILE] [--config PATH]\n\nSend the bot a message first. Uses the selected profile's saved token; an\nomitted profile follows TGS_PROFILE, then the default or only profile. Updates\nare never acknowledged or discarded. Does not work with an active webhook.\nUse tgs setup for guided pairing, or -- before option-like profile names.\n"
        ),
        ("bot", "list") => page!(
            "list saved bots",
            "USAGE:\n    tgs bot list [--config PATH]\n\nLists local profile names and marks the default; never contacts Telegram.\n"
        ),
        ("bot", "use") => page!(
            "select the default bot",
            "USAGE:\n    tgs bot use PROFILE [--config PATH]\n\nSets the saved default. TGS_PROFILE still takes precedence when it is set.\nUse -- before option-like profile names.\n"
        ),
        ("bot", "remove") => page!(
            "remove a local bot profile",
            "USAGE:\n    tgs bot remove PROFILE [--config PATH]\n\nDeletes only the saved profile, not the Telegram bot. If it was the default,\nthe first remaining profile becomes the default. Use -- before option-like names.\n"
        ),
        ("bot", _) => BOT,
        ("config", "init") => page!(
            "initialize a local profile",
            "USAGE:\n    tgs config init [PROFILE] [--config PATH]\n\nCreates an empty profile named default if omitted. Never contacts Telegram.\nUse tgs setup for guided onboarding, or -- before option-like profile names.\n"
        ),
        ("config", "set") => page!(
            "edit a saved profile",
            "USAGE:\n    tgs config set PROFILE [--chat-id ID] [--token-stdin] [--config PATH]\n\nUpdates a destination and/or token without contacting Telegram. With no\ndestination option, reads a hidden token; --token-stdin reads it from stdin.\nOmitted fields are preserved. Use -- before option-like profile names.\n"
        ),
        ("config", "show") => page!(
            "show saved profile metadata",
            "USAGE:\n    tgs config show [PROFILE] [--config PATH]\n\nShows saved metadata, never a token value. An omitted profile follows\nTGS_PROFILE, then the default or only profile. Credential overrides are not\napplied here; use tgs status for effective settings. Use -- before option-like names.\n"
        ),
        ("config", "list") => page!(
            "list local profiles",
            "USAGE:\n    tgs config list [--config PATH]\n\nLists saved profiles and marks the default; never contacts Telegram.\n"
        ),
        ("config", "use") => page!(
            "select the default profile",
            "USAGE:\n    tgs config use PROFILE [--config PATH]\n\nSets the saved default. TGS_PROFILE still takes precedence when it is set.\nUse -- before option-like profile names.\n"
        ),
        ("config", "remove") => page!(
            "remove a local profile",
            "USAGE:\n    tgs config remove PROFILE [--config PATH]\n\nRemoves only a saved profile, not the Telegram bot. If it was the default,\nthe first remaining profile becomes the default. Use -- before option-like names.\n"
        ),
        ("config", "path") => page!(
            "locate configuration",
            "USAGE:\n    tgs config path [--config PATH]\n\nPrints the path selected by --config, TGS_CONFIG, or the platform default.\nDoes not create or read the configuration file.\n"
        ),
        ("config", _) => CONFIG,
        _ => SEND,
    })
}
