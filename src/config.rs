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
const LOCK_FILE_NAME: &str = ".tgs-config.lock";
static NEXT_TEMPORARY_ID: AtomicU64 = AtomicU64::new(0);

/// Schema-versioned local configuration.
#[derive(Clone, Deserialize, Serialize, Eq, PartialEq)]
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
#[derive(Clone, Default, Deserialize, Serialize, Eq, PartialEq)]
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
    /// Resolves an explicit name, configured default, or the only profile.
    /// An empty configuration has no selection; multiple profiles require a choice.
    pub fn selected_name<'a>(
        &'a self,
        requested: Option<&'a str>,
    ) -> Result<Option<&'a str>, TgsError> {
        if let Some(name) = requested.or(self.default_profile.as_deref()) {
            validate_profile_name(name)?;
            return Ok(Some(name));
        }
        match self.profiles.len() {
            0 => Ok(None),
            1 => Ok(self.profiles.keys().next().map(String::as_str)),
            _ => Err(TgsError::config(
                "multiple profiles configured; select one with --profile or `tgs bot use`",
            )),
        }
    }

    /// Loads and validates an existing configuration file.
    pub fn load(path: &Path) -> Result<Self, TgsError> {
        validate_storage_path(path)?;
        let file = File::open(path).map_err(|_| {
            TgsError::config("cannot read configuration; check its path and permissions")
        })?;
        let contents = crate::input::read_text(
            file,
            crate::input::MAX_CONFIG_BYTES,
            crate::error::ErrorKind::Config,
            "configuration",
        )?;
        let result: Self = serde_json::from_str(&contents).map_err(|error| {
            TgsError::config(format!(
                "invalid configuration at line {}, column {}; check the JSON schema",
                error.line(),
                error.column()
            ))
        })?;
        result.validate_contents()?;
        Ok(result)
    }

    /// Loads a configuration or returns an empty schema-v1 value when absent.
    pub fn load_optional(path: &Path) -> Result<Self, TgsError> {
        validate_storage_path(path)?;
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
    /// This is an unconditional replacement. CLI updates use a checked snapshot.
    pub fn save(&self, path: &Path) -> Result<(), TgsError> {
        self.save_checked(path, None)
    }

    // Compare and replace under the same OS lock so two tgs writers cannot both
    // accept a stale snapshot. No lock is held during prompts or network calls.
    pub(crate) fn save_if_unchanged(&self, path: &Path, initial: &Self) -> Result<(), TgsError> {
        self.save_checked(path, Some(initial))
    }

    fn save_checked(&self, path: &Path, initial: Option<&Self>) -> Result<(), TgsError> {
        self.validate_contents()?;
        validate_storage_path(path)?;
        let serialized = serde_json::to_vec_pretty(self)
            .map_err(|_| TgsError::config("cannot encode configuration"))?;
        if serialized.len() >= crate::input::MAX_CONFIG_BYTES {
            return Err(TgsError::config(
                "configuration exceeds the 1 MiB size limit",
            ));
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

        let _lock = lock_directory(parent)?;
        if let Some(initial) = initial {
            if Self::load_optional(path)? != *initial {
                return Err(TgsError::config(
                    "configuration changed during this command; nothing saved, please retry",
                ));
            }
        }
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
            validate_profile_name(default).map_err(|_| {
                TgsError::config("configuration contains an invalid default profile name")
            })?;
            if !self.profiles.contains_key(default) {
                return Err(TgsError::config(
                    "configured default profile does not exist",
                ));
            }
        }
        for (name, profile) in &self.profiles {
            validate_profile_name(name)
                .map_err(|_| TgsError::config("configuration contains an invalid profile name"))?;
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

fn validate_storage_path(path: &Path) -> Result<(), TgsError> {
    let name = path
        .file_name()
        .ok_or_else(|| TgsError::config("configuration path must name a file"))?;
    if name.to_str().is_some_and(|name| {
        name.trim_end_matches(['.', ' '])
            .eq_ignore_ascii_case(LOCK_FILE_NAME)
    }) {
        return Err(TgsError::config(
            "configuration path uses the reserved lock filename",
        ));
    }
    Ok(())
}

struct ConfigurationLock(File);

impl Drop for ConfigurationLock {
    fn drop(&mut self) {
        // Close alone can leave a Unix lock held by a duplicated/inherited
        // handle (including a concurrent fork before exec closes CLOEXEC fds).
        // Explicitly unlock the shared open-file description first.
        let _release = fs2::FileExt::unlock(&self.0);
    }
}

fn lock_directory(parent: &Path) -> Result<ConfigurationLock, TgsError> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(parent.join(LOCK_FILE_NAME)).map_err(|_| {
        TgsError::config("cannot open configuration lock; check directory permissions")
    })?;
    match fs2::FileExt::try_lock_exclusive(&file) {
        Ok(()) => Ok(ConfigurationLock(file)),
        Err(error) if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() => {
            Err(TgsError::config(
                "configuration is busy; another tgs command is saving it, please retry",
            ))
        }
        Err(_) => Err(TgsError::config(
            "cannot lock configuration; check filesystem locking support",
        )),
    }
    // The handle closes on return from the save, releasing the OS lock even if
    // the process exits abnormally. The empty lock file must never be unlinked.
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
    if token.len() > crate::input::MAX_TOKEN_BYTES
        || bot_id.len() < 6
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
        && chat_id.parse::<i64>().is_ok_and(|value| value != 0);
    let username = chat_id.strip_prefix('@').is_some_and(|value| {
        (5..=32).contains(&value.len())
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    });
    if !numeric && !username {
        return Err(TgsError::usage(
            "chat ID must be a nonzero signed 64-bit number or an @username of 5-32 ASCII characters",
        ));
    }
    Ok(())
}

/// Creates an empty named profile and selects it when no default exists.
pub fn init_profile(path: &Path, profile: &str) -> Result<(), TgsError> {
    validate_profile_name(profile)?;
    let mut config = Config::load_optional(path)?;
    let initial = config.clone();
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
    config.save_if_unchanged(path, &initial)
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
    let initial = config.clone();
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
    config.save_if_unchanged(path, &initial)
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
    let profile = config
        .selected_name(requested_profile)?
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
    let initial = config.clone();
    if !config.profiles.contains_key(profile) {
        return Err(TgsError::config(format!(
            "profile `{profile}` does not exist"
        )));
    }
    config.default_profile = Some(profile.to_owned());
    config.save_if_unchanged(path, &initial)
}

/// Removes a profile and selects the first remaining profile when necessary.
pub fn remove_profile(path: &Path, profile: &str) -> Result<(), TgsError> {
    validate_profile_name(profile)?;
    let mut config = Config::load_optional(path)?;
    let initial = config.clone();
    if config.profiles.remove(profile).is_none() {
        return Err(TgsError::config(format!(
            "profile `{profile}` does not exist"
        )));
    }
    if config.default_profile.as_deref() == Some(profile) {
        config.default_profile = config.profiles.keys().next().cloned();
    }
    config.save_if_unchanged(path, &initial)
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
    if let Some(path) = environment.get("TGS_CONFIG")? {
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
            .get("APPDATA")?
            .filter(|root| !root.is_empty())
            .map(|root| PathBuf::from(root).join("tgs").join("config.json"))
            .ok_or_else(|| TgsError::config("APPDATA is not set; use --config or TGS_CONFIG"))
    }
    #[cfg(target_os = "macos")]
    {
        environment
            .get("HOME")?
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
            .get("XDG_CONFIG_HOME")?
            .filter(|root| !root.is_empty())
        {
            return Ok(PathBuf::from(root).join("tgs").join("config.json"));
        }
        environment
            .get("HOME")?
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
            2,
            "only the configuration and persistent empty lock file should remain"
        );
        assert_eq!(
            fs::metadata(directory.join(LOCK_FILE_NAME))
                .expect("lock")
                .len(),
            0
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
        assert_eq!(
            fs::metadata(directory.join(LOCK_FILE_NAME))
                .expect("lock")
                .permissions()
                .mode()
                & 0o777,
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
                "configured default profile does not exist",
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
        for valid in [
            "1",
            "-100123",
            "9223372036854775807",
            "-9223372036854775808",
            "@abcde",
            "@name_with_underscore",
        ] {
            assert!(validate_chat_id(valid).is_ok(), "{valid}");
        }
        for invalid in [
            "",
            "0",
            "-0",
            "@abcd",
            "with space",
            "+123",
            "9223372036854775808",
            "-9223372036854775809",
            "999999999999999999999999999999999999999999",
            "１２３",
        ] {
            assert!(validate_chat_id(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn checked_saves_reject_stale_snapshots_without_losing_updates() {
        let directory = test_path("snapshot-conflict");
        let path = directory.join("config.json");
        let initial = Config::default();
        initial.save(&path).expect("initial");
        let mut pending = initial.clone();
        pending
            .profiles
            .insert("pending".to_owned(), Profile::default());
        init_profile(&path, "concurrent").expect("other writer");
        let before = fs::read(&path).expect("before");
        let error = pending
            .save_if_unchanged(&path, &initial)
            .expect_err("conflict");
        assert_eq!(error.exit_code(), 3);
        assert!(error.message().contains("changed during this command"));
        assert_eq!(fs::read(&path).expect("after"), before);
        assert!(
            Config::load(&path)
                .expect("load")
                .profiles
                .contains_key("concurrent")
        );
        fs::remove_dir_all(directory).expect("cleanup");
    }

    #[test]
    fn busy_configuration_fails_without_writing_and_unlocks_on_close() {
        let directory = test_path("busy-lock");
        let path = directory.join("config.json");
        Config::default().save(&path).expect("initial");
        let before = fs::read(&path).expect("before");
        let lock = lock_directory(&directory).expect("hold lock");
        let error = init_profile(&path, "pending").expect_err("busy");
        assert_eq!(error.exit_code(), 3);
        assert!(error.message().contains("configuration is busy"));
        assert_eq!(fs::read(&path).expect("after"), before);
        drop(lock);
        init_profile(&path, "pending").expect("lock released");
        assert_eq!(
            fs::metadata(directory.join(LOCK_FILE_NAME))
                .expect("lock")
                .len(),
            0
        );
        fs::remove_dir_all(directory).expect("cleanup");
    }

    #[test]
    fn releasing_a_configuration_lock_does_not_wait_for_duplicated_handles() {
        let directory = test_path("duplicated-lock");
        fs::create_dir(&directory).expect("directory");
        let lock = lock_directory(&directory).expect("first lock");
        let duplicate = lock.0.try_clone().expect("duplicate handle");
        drop(lock);
        let next = lock_directory(&directory).expect("released despite live duplicate");
        drop(next);
        drop(duplicate);
        fs::remove_dir_all(directory).expect("cleanup");
    }

    #[test]
    fn reserved_lock_filename_is_rejected_before_creating_state() {
        let directory = test_path("reserved-lock");
        for name in [LOCK_FILE_NAME, ".TGS-CONFIG.LOCK", ".tgs-config.lock. "] {
            let error = Config::default()
                .save(&directory.join(name))
                .expect_err("reserved");
            assert_eq!(error.exit_code(), 3);
        }
        assert!(!directory.exists());
    }

    #[test]
    fn malformed_configuration_never_echoes_unvalidated_contents() {
        let directory = test_path("secret-errors");
        fs::create_dir_all(&directory).expect("directory");
        let path = directory.join("config.json");
        for contents in [
            r#"{"version":"PRIVATE_VALUE","profiles":{}}"#,
            r#"{"version":1,"profiles":{},"PRIVATE_VALUE":true}"#,
            r#"{"version":1,"default_profile":"PRIVATE_VALUE","profiles":{}}"#,
            r#"{"version":1,"profiles":{"PRIVATE_VALUE:secret":{"token":""}}}"#,
            r#"{"version":1,"default_profile":"PRIVATE_VALUE:secret","profiles":{}}"#,
        ] {
            fs::write(&path, contents).expect("fixture");
            let error = Config::load(&path).expect_err("invalid config");
            assert_eq!(error.exit_code(), 3);
            assert!(!error.to_string().contains("PRIVATE_VALUE"));
            assert!(!format!("{error:?}").contains("PRIVATE_VALUE"));
        }
        fs::remove_dir_all(directory).expect("cleanup");
    }

    #[test]
    fn oversized_configuration_is_rejected_before_filesystem_changes() {
        let directory = test_path("oversized-config");
        let mut config = Config::default();
        let token = format!("000000:{}", "a".repeat(4000));
        for index in 0..300 {
            config.profiles.insert(
                format!("profile-{index}"),
                Profile {
                    token: token.clone(),
                    chat_id: None,
                },
            );
        }
        let error = config
            .save(&directory.join("config.json"))
            .expect_err("size cap");
        assert_eq!(error.exit_code(), 3);
        assert!(error.message().contains("1 MiB"));
        assert!(!directory.exists());
        assert!(validate_token(&format!("000000:{}", "a".repeat(4096))).is_err());
    }
}
