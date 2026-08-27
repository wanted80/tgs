use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::{Environment, error::TgsError};

const CONFIG_VERSION: u8 = 1;
static NEXT_TEMPORARY_ID: AtomicU64 = AtomicU64::new(0);

/// Schema-versioned local configuration.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Configuration schema version.
    pub version: u8,
    /// Default profile used when no override is supplied.
    pub default_profile: Option<String>,
    /// Named token and destination profiles.
    pub profiles: BTreeMap<String, Profile>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            default_profile: None,
            profiles: BTreeMap::new(),
        }
    }
}

/// Secret-bearing Telegram token and optional default destination.
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// Telegram bot token. This value must never be displayed.
    pub token: String,
    /// Optional default chat, group, or channel destination.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<String>,
}

/// Non-secret profile metadata returned by configuration queries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileInfo {
    /// Local profile name.
    pub name: String,
    /// Whether this profile is selected by default.
    pub is_default: bool,
    /// Optional configured Telegram destination.
    pub chat_id: Option<String>,
    /// Whether a token is configured, without exposing its value.
    pub token_is_set: bool,
}

impl Config {
    /// Loads and validates an existing configuration file.
    pub fn load(path: &Path) -> Result<Self, TgsError> {
        let contents = fs::read_to_string(path).map_err(|error| {
            TgsError::config(format!(
                "cannot read configuration `{}`: {error}",
                path.display()
            ))
        })?;
        let result: Self = serde_json::from_str(&contents).map_err(|error| {
            TgsError::config(format!(
                "invalid configuration `{}`: {error}",
                path.display()
            ))
        })?;
        result.validate_contents()?;
        Ok(result)
    }

    /// Loads a configuration or returns an empty schema-v1 value when absent.
    pub fn load_optional(path: &Path) -> Result<Self, TgsError> {
        match path.try_exists() {
            Ok(true) => Self::load(path),
            Ok(false) => Ok(Self::default()),
            Err(error) => Err(TgsError::config(format!(
                "cannot access configuration `{}`: {error}",
                path.display()
            ))),
        }
    }

    /// Atomically replaces the configuration with restrictive permissions where supported.
    pub fn save(&self, path: &Path) -> Result<(), TgsError> {
        self.validate_contents()?;
        if path.file_name().is_none() {
            return Err(TgsError::config(format!(
                "configuration path `{}` does not name a file",
                path.display()
            )));
        }
        let parent = path
            .parent()
            .filter(|value| !value.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let parent_existed = parent.try_exists().map_err(|error| {
            TgsError::config(format!(
                "cannot access configuration directory `{}`: {error}",
                parent.display()
            ))
        })?;
        fs::create_dir_all(parent).map_err(|error| {
            TgsError::config(format!(
                "cannot create configuration directory `{}`: {error}",
                parent.display()
            ))
        })?;
        if !parent_existed {
            set_directory_permissions(parent)?;
        }

        let serialized = serde_json::to_vec_pretty(self)
            .map_err(|error| TgsError::config(format!("cannot encode configuration: {error}")))?;
        let (temporary, mut file) = create_temporary_file(parent)?;
        let write_result = (|| -> std::io::Result<()> {
            file.write_all(&serialized)?;
            file.write_all(b"\n")?;
            file.sync_all()
        })();
        if let Err(error) = write_result {
            let _ignored = fs::remove_file(&temporary);
            return Err(TgsError::config(format!(
                "cannot write configuration `{}`: {error}",
                path.display()
            )));
        }
        drop(file);
        fs::rename(&temporary, path).map_err(|error| {
            let _ignored = fs::remove_file(&temporary);
            TgsError::config(format!(
                "cannot replace configuration `{}`: {error}",
                path.display()
            ))
        })
    }

    fn validate_contents(&self) -> Result<(), TgsError> {
        if self.version != CONFIG_VERSION {
            return Err(TgsError::config(format!(
                "unsupported configuration version {}; expected {CONFIG_VERSION}",
                self.version
            )));
        }
        if let Some(default) = self.default_profile.as_deref() {
            if !self.profiles.contains_key(default) {
                return Err(TgsError::config(format!(
                    "default profile `{default}` does not exist"
                )));
            }
        }
        for (name, profile) in &self.profiles {
            validate_profile_name(name).map_err(|_| {
                TgsError::config(format!(
                    "configuration contains invalid profile name `{name}`"
                ))
            })?;
            if !profile.token.is_empty() {
                validate_token(&profile.token).map_err(|_| {
                    TgsError::config(format!(
                        "profile `{name}` contains an invalid Telegram token"
                    ))
                })?;
            }
            if let Some(chat_id) = profile.chat_id.as_deref() {
                validate_chat_id(chat_id).map_err(|_| {
                    TgsError::config(format!("profile `{name}` contains an invalid chat ID"))
                })?;
            }
        }
        Ok(())
    }
}

impl fmt::Debug for Config {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Config")
            .field("version", &self.version)
            .field("default_profile_is_set", &self.default_profile.is_some())
            .field("profile_count", &self.profiles.len())
            .finish()
    }
}

impl fmt::Debug for Profile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let token = if self.token.is_empty() {
            "<not set>"
        } else {
            "<redacted>"
        };
        formatter
            .debug_struct("Profile")
            .field("token", &token)
            .field("chat_id_is_set", &self.chat_id.is_some())
            .finish()
    }
}

fn create_temporary_file(parent: &Path) -> Result<(PathBuf, File), TgsError> {
    const MAX_ATTEMPTS: usize = 100;
    for _ in 0..MAX_ATTEMPTS {
        let id = NEXT_TEMPORARY_ID.fetch_add(1, Ordering::Relaxed);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = parent.join(format!(
            ".tgs-config-{}-{timestamp}-{id}.tmp",
            std::process::id()
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(TgsError::config(format!(
                    "cannot create temporary configuration in `{}`: {error}",
                    parent.display()
                )));
            }
        }
    }
    Err(TgsError::config(format!(
        "cannot allocate a temporary configuration file in `{}`",
        parent.display()
    )))
}

/// Validates a local profile name without filesystem side effects.
pub fn validate_profile_name(name: &str) -> Result<(), TgsError> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
    {
        return Err(TgsError::usage(
            "profile names must be 1-64 ASCII letters, digits, dots, underscores, or hyphens",
        ));
    }
    Ok(())
}

/// Validates a Telegram bot token without including it in any error.
pub fn validate_token(token: &str) -> Result<(), TgsError> {
    let Some((bot_id, secret)) = token.split_once(':') else {
        return Err(TgsError::usage("invalid Telegram bot token format"));
    };
    if bot_id.len() < 6
        || !bot_id.bytes().all(|byte| byte.is_ascii_digit())
        || secret.len() < 20
        || !secret
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(TgsError::usage("invalid Telegram bot token format"));
    }
    Ok(())
}

/// Validates a nonzero numeric chat ID or an `@username` destination.
pub fn validate_chat_id(chat_id: &str) -> Result<(), TgsError> {
    let numeric_value = chat_id.strip_prefix('-').unwrap_or(chat_id);
    let numeric = !numeric_value.is_empty()
        && numeric_value.bytes().all(|byte| byte.is_ascii_digit())
        && numeric_value.bytes().any(|byte| byte != b'0');
    let username = chat_id.strip_prefix('@').is_some_and(|value| {
        (5..=32).contains(&value.len())
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    });
    if !numeric && !username {
        return Err(TgsError::usage(
            "chat ID must be a nonzero number or an @username of 5-32 ASCII characters",
        ));
    }
    Ok(())
}

/// Creates an empty named profile and selects it when no default exists.
pub fn init_profile(path: &Path, profile: &str) -> Result<(), TgsError> {
    validate_profile_name(profile)?;
    let mut config = Config::load_optional(path)?;
    if config.profiles.contains_key(profile) {
        return Err(TgsError::usage(format!(
            "profile `{profile}` already exists"
        )));
    }
    config
        .profiles
        .insert(profile.to_owned(), Profile::default());
    if config.default_profile.is_none() {
        config.default_profile = Some(profile.to_owned());
    }
    config.save(path)
}

/// Updates a profile token, destination, or both.
pub fn set_profile(
    path: &Path,
    profile: &str,
    chat_id: Option<&str>,
    token: Option<&str>,
) -> Result<(), TgsError> {
    validate_profile_name(profile)?;
    if chat_id.is_none() && token.is_none() {
        return Err(TgsError::usage(
            "profile update requires a token, a chat ID, or both",
        ));
    }
    if let Some(value) = chat_id {
        validate_chat_id(value)?;
    }
    if let Some(value) = token {
        validate_token(value)?;
    }

    let mut config = Config::load_optional(path)?;
    let entry = config.profiles.entry(profile.to_owned()).or_default();
    if let Some(value) = token {
        entry.token = value.to_owned();
    }
    if let Some(value) = chat_id {
        entry.chat_id = Some(value.to_owned());
    }
    if config.default_profile.is_none() {
        config.default_profile = Some(profile.to_owned());
    }
    config.save(path)
}

/// Lists configured profiles in deterministic name order.
pub fn list_profiles(path: &Path) -> Result<Vec<(String, bool)>, TgsError> {
    let config = Config::load_optional(path)?;
    Ok(config
        .profiles
        .keys()
        .map(|name| {
            (
                name.clone(),
                config.default_profile.as_deref() == Some(name),
            )
        })
        .collect())
}

/// Returns redacted metadata for an explicit or default profile.
pub fn profile_info(path: &Path, requested_profile: Option<&str>) -> Result<ProfileInfo, TgsError> {
    if let Some(profile) = requested_profile {
        validate_profile_name(profile)?;
    }
    let config = Config::load_optional(path)?;
    let profile = requested_profile
        .or(config.default_profile.as_deref())
        .ok_or_else(|| TgsError::config("no profile selected"))?;
    let item = config
        .profiles
        .get(profile)
        .ok_or_else(|| TgsError::config(format!("profile `{profile}` does not exist")))?;
    Ok(ProfileInfo {
        name: profile.to_owned(),
        is_default: config.default_profile.as_deref() == Some(profile),
        chat_id: item.chat_id.clone(),
        token_is_set: !item.token.is_empty(),
    })
}

/// Selects an existing profile as the default.
pub fn use_profile(path: &Path, profile: &str) -> Result<(), TgsError> {
    validate_profile_name(profile)?;
    let mut config = Config::load_optional(path)?;
    if !config.profiles.contains_key(profile) {
        return Err(TgsError::config(format!(
            "profile `{profile}` does not exist"
        )));
    }
    config.default_profile = Some(profile.to_owned());
    config.save(path)
}

/// Removes a profile and selects the first remaining profile when necessary.
pub fn remove_profile(path: &Path, profile: &str) -> Result<(), TgsError> {
    validate_profile_name(profile)?;
    let mut config = Config::load_optional(path)?;
    if config.profiles.remove(profile).is_none() {
        return Err(TgsError::config(format!(
            "profile `{profile}` does not exist"
        )));
    }
    if config.default_profile.as_deref() == Some(profile) {
        config.default_profile = config.profiles.keys().next().cloned();
    }
    config.save(path)
}

/// Resolves an explicit, environment, or native-platform configuration path.
pub fn resolve_config_path(
    explicit: Option<&Path>,
    environment: &Environment,
) -> Result<PathBuf, TgsError> {
    if let Some(path) = explicit {
        if path.as_os_str().is_empty() {
            return Err(TgsError::usage("configuration path must not be empty"));
        }
        return Ok(path.to_path_buf());
    }
    if let Some(path) = environment.get("TGS_CONFIG") {
        if path.is_empty() {
            return Err(TgsError::config("TGS_CONFIG must not be empty"));
        }
        return Ok(PathBuf::from(path));
    }
    platform_config_path(environment)
}

fn platform_config_path(environment: &Environment) -> Result<PathBuf, TgsError> {
    #[cfg(target_os = "windows")]
    {
        environment
            .get("APPDATA")
            .filter(|root| !root.is_empty())
            .map(|root| PathBuf::from(root).join("tgs").join("config.json"))
            .ok_or_else(|| TgsError::config("APPDATA is not set; use --config or TGS_CONFIG"))
    }
    #[cfg(target_os = "macos")]
    {
        environment
            .get("HOME")
            .filter(|root| !root.is_empty())
            .map(|root| {
                PathBuf::from(root)
                    .join("Library")
                    .join("Application Support")
                    .join("tgs")
                    .join("config.json")
            })
            .ok_or_else(|| TgsError::config("HOME is not set; use --config or TGS_CONFIG"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(root) = environment
            .get("XDG_CONFIG_HOME")
            .filter(|root| !root.is_empty())
        {
            return Ok(PathBuf::from(root).join("tgs").join("config.json"));
        }
        environment
            .get("HOME")
            .filter(|root| !root.is_empty())
            .map(|root| {
                PathBuf::from(root)
                    .join(".config")
                    .join("tgs")
                    .join("config.json")
            })
            .ok_or_else(|| TgsError::config("HOME is not set; use --config or TGS_CONFIG"))
    }
}

#[cfg(unix)]
fn set_directory_permissions(path: &Path) -> Result<(), TgsError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| {
        TgsError::config(format!(
            "cannot secure configuration directory `{}`: {error}",
            path.display()
        ))
    })
}

#[cfg(not(unix))]
fn set_directory_permissions(_path: &Path) -> Result<(), TgsError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    const TOKEN: &str = "000000:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn test_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("tgs-{name}-{}-{nonce}", std::process::id()))
    }

    #[test]
    fn config_round_trip_does_not_change_values() {
        let directory = test_path("roundtrip");
        let path = directory.join("config.json");
        let mut config = Config {
            default_profile: Some("ops".to_owned()),
            ..Config::default()
        };
        config.profiles.insert(
            "ops".to_owned(),
            Profile {
                token: TOKEN.to_owned(),
                chat_id: Some("@alerts".to_owned()),
            },
        );
        config.save(&path).expect("save");
        let loaded = Config::load(&path).expect("load");
        assert_eq!(loaded.default_profile.as_deref(), Some("ops"));
        assert_eq!(loaded.profiles["ops"].token, TOKEN);
        fs::remove_dir_all(directory).expect("cleanup");
    }

    #[test]
    fn save_atomically_replaces_an_existing_configuration() {
        let directory = test_path("replace");
        let path = directory.join("config.json");
        Config::default().save(&path).expect("first save");

        let mut replacement = Config {
            default_profile: Some("ops".to_owned()),
            ..Config::default()
        };
        replacement.profiles.insert(
            "ops".to_owned(),
            Profile {
                token: TOKEN.to_owned(),
                chat_id: Some("-100123".to_owned()),
            },
        );
        replacement.save(&path).expect("replacement save");

        let loaded = Config::load(&path).expect("load replacement");
        assert_eq!(loaded.default_profile.as_deref(), Some("ops"));
        assert_eq!(loaded.profiles["ops"].chat_id.as_deref(), Some("-100123"));
        assert_eq!(
            fs::read_dir(&directory).expect("directory entries").count(),
            1,
            "temporary file must not remain after replacement"
        );
        fs::remove_dir_all(directory).expect("cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn save_uses_restrictive_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let directory = test_path("permissions");
        let path = directory.join("config.json");
        Config::default().save(&path).expect("save");
        assert_eq!(
            fs::metadata(&directory)
                .expect("directory")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&path).expect("file").permissions().mode() & 0o777,
            0o600
        );
        fs::remove_dir_all(directory).expect("cleanup");
    }

    #[test]
    fn explicit_path_precedes_environment() {
        let environment = Environment::from_values([(
            "TGS_CONFIG".to_owned(),
            "/environment/config.json".to_owned(),
        )]);
        let result =
            resolve_config_path(Some(Path::new("explicit.json")), &environment).expect("path");
        assert_eq!(result, Path::new("explicit.json"));
    }

    #[test]
    fn rejects_unsafe_profile_names() {
        for name in ["", "with space", "../escape", "emoji-😀"] {
            assert!(validate_profile_name(name).is_err(), "{name}");
        }
    }

    #[test]
    fn rejects_malformed_configuration_invariants() {
        let directory = test_path("malformed");
        fs::create_dir_all(&directory).expect("create directory");
        for (name, contents, expected) in [
            (
                "missing-default.json",
                r#"{"version":1,"default_profile":"missing","profiles":{}}"#,
                "default profile `missing` does not exist",
            ),
            (
                "bad-token.json",
                r#"{"version":1,"default_profile":"ops","profiles":{"ops":{"token":"secret","chat_id":"1"}}}"#,
                "profile `ops` contains an invalid Telegram token",
            ),
            (
                "bad-chat.json",
                r#"{"version":1,"default_profile":"ops","profiles":{"ops":{"token":"","chat_id":"with space"}}}"#,
                "profile `ops` contains an invalid chat ID",
            ),
        ] {
            let path = directory.join(name);
            fs::write(&path, contents).expect("write malformed config");
            let error = Config::load(&path).expect_err("reject malformed config");
            assert_eq!(error.kind(), crate::error::ErrorKind::Config);
            assert_eq!(error.message(), expected);
        }
        fs::remove_dir_all(directory).expect("cleanup");
    }

    #[test]
    fn invalid_configuration_is_rejected_before_creating_a_directory() {
        let directory = test_path("no-side-effect");
        let path = directory.join("config.json");
        let config = Config {
            version: 2,
            ..Config::default()
        };
        let error = config.save(&path).expect_err("reject version");
        assert!(
            error
                .message()
                .contains("unsupported configuration version")
        );
        assert!(!directory.exists());
    }

    #[test]
    fn debug_output_redacts_profile_tokens() {
        let profile = Profile {
            token: TOKEN.to_owned(),
            chat_id: Some("123".to_owned()),
        };
        let output = format!("{profile:?}");
        assert!(output.contains("<redacted>"));
        assert!(!output.contains(TOKEN));
        assert!(!output.contains("123"));

        let mut config = Config::default();
        config.profiles.insert("ops".to_owned(), profile);
        let output = format!("{config:?}");
        assert!(!output.contains(TOKEN));
        assert!(!output.contains("ops"));
    }

    #[test]
    fn token_and_destination_validation_cover_boundaries() {
        assert!(validate_token(TOKEN).is_ok());
        for invalid in [
            "",
            "12345:aaaaaaaaaaaaaaaaaaaa",
            "000000:too-short",
            "000000:contains whitespace here",
        ] {
            assert_eq!(
                validate_token(invalid)
                    .expect_err("invalid token")
                    .message(),
                "invalid Telegram bot token format"
            );
        }
        for valid in ["1", "-100123", "@abcde", "@name_with_underscore"] {
            assert!(validate_chat_id(valid).is_ok(), "{valid}");
        }
        for invalid in ["", "0", "-0", "@abcd", "with space", "+123"] {
            assert!(validate_chat_id(invalid).is_err(), "{invalid}");
        }
    }
}
