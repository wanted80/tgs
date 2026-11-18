//! Friendly local bot-profile management.

use std::path::Path;

use crate::config::{self, Config, Profile, validate_profile_name};
pub use crate::config::{validate_chat_id, validate_token};
use crate::error::TgsError;
use crate::telegram::BotIdentity;

const BOT_REQUEST_TIMEOUT_SECONDS: u64 = 10;

/// Live identity lookup used to validate a token without coupling tests to Telegram.
pub trait BotIdentityLookup {
    /// Returns the Telegram identity associated with `token`.
    fn get_me(&self, token: &str) -> Result<BotIdentity, TgsError>;
}

/// Recent-chat lookup used for destination discovery without exposing a token.
pub trait BotChatLookup {
    /// Returns deduplicated chats observed in recent Telegram updates.
    fn recent_chats(&self, token: &str) -> Result<Vec<BotChat>, TgsError>;
}

/// A redacted Telegram destination discovered from recent bot updates.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BotChat {
    /// Stable Telegram chat identifier accepted by `--chat-id`.
    pub id: i64,
    /// Telegram chat type such as `private`, `group`, or `channel`.
    pub kind: String,
    /// Human-readable, single-line chat label.
    pub name: String,
}

/// Recent chats together with the local profile used for the lookup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BotChats {
    /// Resolved local profile name.
    pub profile: String,
    /// Deduplicated recent chats in stable ID order.
    pub chats: Vec<BotChat>,
}

/// Redacted local profile state together with its live Telegram identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BotInfo {
    /// Local profile name.
    pub profile: String,
    /// Whether this profile is the configured default.
    pub is_default: bool,
    /// Optional configured destination.
    pub chat_id: Option<String>,
    /// Live bot identity returned by Telegram.
    pub identity: BotIdentity,
}

/// Validates that a new profile can be created before interactive token input.
pub fn validate_create(path: &Path, profile: &str, chat_id: Option<&str>) -> Result<(), TgsError> {
    validate_profile_name(profile)?;
    if let Some(value) = chat_id {
        validate_chat_id(value)?;
    }
    let config = Config::load_optional(path)?;
    if config.profiles.contains_key(profile) {
        return Err(TgsError::usage(format!(
            "profile `{profile}` already exists"
        )));
    }
    Ok(())
}

/// Validates a token through Telegram and atomically adds or updates a profile.
///
/// When `require_absent` is true, an existing profile is rejected. When replacing
/// a profile, an omitted `chat_id` preserves its current destination.
pub fn add<T: BotIdentityLookup>(
    path: &Path,
    profile: &str,
    chat_id: Option<&str>,
    token: &str,
    require_absent: bool,
    transport: &T,
) -> Result<BotIdentity, TgsError> {
    validate_profile_name(profile)?;
    validate_token(token)?;
    if let Some(value) = chat_id {
        validate_chat_id(value)?;
    }

    let mut config = Config::load_optional(path)?;
    let initial = config.clone();
    if require_absent && config.profiles.contains_key(profile) {
        return Err(TgsError::usage(format!(
            "profile `{profile}` already exists"
        )));
    }

    let identity = transport.get_me(token)?;
    let preserved_chat_id = config
        .profiles
        .get(profile)
        .and_then(|item| item.chat_id.clone());
    config.profiles.insert(
        profile.to_owned(),
        Profile {
            token: token.to_owned(),
            chat_id: chat_id.map(str::to_owned).or(preserved_chat_id),
        },
    );
    if config.default_profile.is_none() {
        config.default_profile = Some(profile.to_owned());
    }
    config.save_if_unchanged(path, &initial)?;
    Ok(identity)
}

/// Adds or updates a profile using the production Telegram API.
pub fn add_live(
    path: &Path,
    profile: &str,
    chat_id: Option<&str>,
    token: &str,
    require_absent: bool,
) -> Result<BotIdentity, TgsError> {
    let transport = crate::telegram::TelegramClient::new(
        crate::telegram::API_BASE,
        BOT_REQUEST_TIMEOUT_SECONDS,
    )?;
    add(path, profile, chat_id, token, require_absent, &transport)
}

/// Returns local profile names and whether each is the default.
pub fn list(path: &Path) -> Result<Vec<(String, bool)>, TgsError> {
    config::list_profiles(path)
}

/// Resolves a profile, then fetches its live Telegram identity.
pub fn info<T: BotIdentityLookup>(
    path: &Path,
    requested_profile: Option<&str>,
    transport: &T,
) -> Result<BotInfo, TgsError> {
    let config = Config::load_optional(path)?;
    let profile = config
        .selected_name(requested_profile)?
        .ok_or_else(|| TgsError::config("no profile selected"))?;
    validate_profile_name(profile)?;
    let local = config
        .profiles
        .get(profile)
        .ok_or_else(|| TgsError::config(format!("profile `{profile}` does not exist")))?;
    validate_token(&local.token)?;
    let identity = transport.get_me(&local.token)?;
    Ok(BotInfo {
        profile: profile.to_owned(),
        is_default: config.default_profile.as_deref() == Some(profile),
        chat_id: local.chat_id.clone(),
        identity,
    })
}

/// Resolves a profile and fetches its identity from the production Telegram API.
pub fn info_live(path: &Path, requested_profile: Option<&str>) -> Result<BotInfo, TgsError> {
    let transport = crate::telegram::TelegramClient::new(
        crate::telegram::API_BASE,
        BOT_REQUEST_TIMEOUT_SECONDS,
    )?;
    info(path, requested_profile, &transport)
}

/// Resolves a profile, then lists chats observed in recent Telegram updates.
pub fn chats<T: BotChatLookup>(
    path: &Path,
    requested_profile: Option<&str>,
    transport: &T,
) -> Result<BotChats, TgsError> {
    let config = Config::load_optional(path)?;
    let profile = config
        .selected_name(requested_profile)?
        .ok_or_else(|| TgsError::config("no profile selected"))?;
    validate_profile_name(profile)?;
    let local = config
        .profiles
        .get(profile)
        .ok_or_else(|| TgsError::config(format!("profile `{profile}` does not exist")))?;
    validate_token(&local.token)?;
    Ok(BotChats {
        profile: profile.to_owned(),
        chats: transport.recent_chats(&local.token)?,
    })
}

/// Lists recent chats using the production Telegram API.
pub fn chats_live(path: &Path, requested_profile: Option<&str>) -> Result<BotChats, TgsError> {
    let transport = crate::telegram::TelegramClient::new(
        crate::telegram::API_BASE,
        BOT_REQUEST_TIMEOUT_SECONDS,
    )?;
    chats(path, requested_profile, &transport)
}

/// Sets an existing profile as the default.
pub fn use_profile(path: &Path, profile: &str) -> Result<(), TgsError> {
    config::use_profile(path, profile)
}

/// Removes only the local profile and leaves the Telegram bot untouched.
pub fn remove(path: &Path, profile: &str) -> Result<(), TgsError> {
    config::remove_profile(path, profile)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    const TOKEN: &str = "000000:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    struct Lookup {
        succeeds: bool,
    }

    impl BotIdentityLookup for Lookup {
        fn get_me(&self, _token: &str) -> Result<BotIdentity, TgsError> {
            if self.succeeds {
                Ok(BotIdentity {
                    id: 42,
                    display_name: "Test Bot".to_owned(),
                    username: Some("test_bot".to_owned()),
                })
            } else {
                Err(TgsError::telegram("rejected by test transport"))
            }
        }
    }

    struct ChatsLookup;

    impl BotChatLookup for ChatsLookup {
        fn recent_chats(&self, token: &str) -> Result<Vec<BotChat>, TgsError> {
            assert_eq!(token, TOKEN);
            Ok(vec![BotChat {
                id: 123,
                kind: "private".to_owned(),
                name: "Test User".to_owned(),
            }])
        }
    }

    fn test_path(name: &str) -> (PathBuf, PathBuf) {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("tgs-bot-{name}-{}-{nonce}", std::process::id()));
        (directory.clone(), directory.join("config.json"))
    }

    #[test]
    fn validation_failure_does_not_modify_configuration() {
        let (directory, path) = test_path("no-write");
        let mut initial = Config::default();
        initial.profiles.insert(
            "ops".to_owned(),
            Profile {
                token: TOKEN.to_owned(),
                chat_id: Some("123".to_owned()),
            },
        );
        initial.save(&path).expect("initial save");
        let before = fs::read(&path).expect("before");

        let result = add(
            &path,
            "ops",
            Some("not a destination"),
            TOKEN,
            false,
            &Lookup { succeeds: true },
        );
        assert!(result.is_err());
        assert_eq!(fs::read(&path).expect("after"), before);
        fs::remove_dir_all(directory).expect("cleanup");
    }

    #[test]
    fn rejected_token_does_not_modify_configuration() {
        let (directory, path) = test_path("transport-failure");
        Config::default().save(&path).expect("initial save");
        let before = fs::read(&path).expect("before");
        let result = add(
            &path,
            "ops",
            None,
            TOKEN,
            false,
            &Lookup { succeeds: false },
        );
        assert!(result.is_err());
        assert_eq!(fs::read(&path).expect("after"), before);
        fs::remove_dir_all(directory).expect("cleanup");
    }

    #[test]
    fn replacing_token_preserves_omitted_destination() {
        let (directory, path) = test_path("preserve-chat");
        let mut initial = Config::default();
        initial.profiles.insert(
            "ops".to_owned(),
            Profile {
                token: "000000:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned(),
                chat_id: Some("-100123".to_owned()),
            },
        );
        initial.save(&path).expect("initial save");

        add(&path, "ops", None, TOKEN, false, &Lookup { succeeds: true }).expect("replace");
        let saved = Config::load(&path).expect("load");
        assert_eq!(saved.profiles["ops"].token, TOKEN);
        assert_eq!(saved.profiles["ops"].chat_id.as_deref(), Some("-100123"));
        fs::remove_dir_all(directory).expect("cleanup");
    }

    #[test]
    fn create_refuses_duplicate_before_lookup() {
        let (directory, path) = test_path("duplicate");
        let mut initial = Config::default();
        initial
            .profiles
            .insert("ops".to_owned(), Profile::default());
        initial.save(&path).expect("initial save");
        let result = add(&path, "ops", None, TOKEN, true, &Lookup { succeeds: true });
        assert!(result.is_err());
        fs::remove_dir_all(directory).expect("cleanup");
    }

    #[test]
    fn validates_unicode_names_and_destination_boundaries() {
        assert!(validate_profile_name("déploy").is_err());
        assert!(validate_chat_id("@abcd").is_err());
        assert!(validate_chat_id("@abcde").is_ok());
        assert!(validate_chat_id("-100123").is_ok());
        assert!(validate_chat_id("-").is_err());
        assert!(validate_token("000000:contains whitespace here").is_err());
    }

    #[test]
    fn recent_chats_use_the_resolved_profile_without_exposing_its_token() {
        let (directory, path) = test_path("chats");
        let mut config = Config {
            default_profile: Some("ops".to_owned()),
            ..Config::default()
        };
        config.profiles.insert(
            "ops".to_owned(),
            Profile {
                token: TOKEN.to_owned(),
                chat_id: None,
            },
        );
        config.save(&path).expect("save");

        let result = chats(&path, None, &ChatsLookup).expect("recent chats");
        assert_eq!(result.profile, "ops");
        assert_eq!(result.chats[0].id, 123);
        assert!(!format!("{result:?}").contains(TOKEN));
        fs::remove_dir_all(directory).expect("cleanup");
    }
}
