use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Environment, error::TgsError};

const CONFIG_VERSION: u8 = 1;

/// Schema-versioned local configuration.
#[derive(Clone, Debug, Deserialize, Serialize)]
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
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// Telegram bot token. This value must never be displayed.
    pub token: String,
    /// Optional default chat, group, or channel destination.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<String>,
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
        if result.version != CONFIG_VERSION {
            return Err(TgsError::config(format!(
                "unsupported configuration version {}; expected {CONFIG_VERSION}",
                result.version
            )));
        }
        for name in result.profiles.keys() {
            validate_profile_name(name)?;
        }
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
        let parent = path.parent().ok_or_else(|| {
            TgsError::config(format!(
                "configuration path `{}` has no parent",
                path.display()
            ))
        })?;
        let parent_existed = parent.exists();
        fs::create_dir_all(parent).map_err(|error| {
            TgsError::config(format!(
                "cannot create configuration directory `{}`: {error}",
                parent.display()
            ))
        })?;
        if !parent_existed {
            set_directory_permissions(parent)?;
        }

        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("config.json");
        let temporary = parent.join(format!(".{file_name}.{}.tmp", std::process::id()));
        let serialized = serde_json::to_vec_pretty(self)
            .map_err(|error| TgsError::config(format!("cannot encode configuration: {error}")))?;
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary).map_err(|error| {
            TgsError::config(format!(
                "cannot create temporary configuration in `{}`: {error}",
                parent.display()
            ))
        })?;
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
        #[cfg(windows)]
        if path.exists() {
            fs::remove_file(path).map_err(|error| {
                TgsError::config(format!(
                    "cannot replace configuration `{}`: {error}",
                    path.display()
                ))
            })?;
        }
        fs::rename(&temporary, path).map_err(|error| {
            let _ignored = fs::remove_file(&temporary);
            TgsError::config(format!(
                "cannot replace configuration `{}`: {error}",
                path.display()
            ))
        })?;
        set_file_permissions(path)
    }
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

/// Resolves an explicit, environment, or native-platform configuration path.
pub fn resolve_config_path(
    explicit: Option<&Path>,
    environment: &Environment,
) -> Result<PathBuf, TgsError> {
    if let Some(path) = explicit {
        return Ok(path.to_path_buf());
    }
    if let Some(path) = environment.get("TGS_CONFIG") {
        return Ok(PathBuf::from(path));
    }
    platform_config_path(environment)
}

fn platform_config_path(environment: &Environment) -> Result<PathBuf, TgsError> {
    #[cfg(target_os = "windows")]
    {
        environment
            .get("APPDATA")
            .map(|root| PathBuf::from(root).join("tgs").join("config.json"))
            .ok_or_else(|| TgsError::config("APPDATA is not set; use --config or TGS_CONFIG"))
    }
    #[cfg(target_os = "macos")]
    {
        environment
            .get("HOME")
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
        if let Some(root) = environment.get("XDG_CONFIG_HOME") {
            return Ok(PathBuf::from(root).join("tgs").join("config.json"));
        }
        environment
            .get("HOME")
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

#[cfg(unix)]
fn set_file_permissions(path: &Path) -> Result<(), TgsError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| {
        TgsError::config(format!(
            "cannot secure configuration `{}`: {error}",
            path.display()
        ))
    })
}

#[cfg(not(unix))]
fn set_file_permissions(_path: &Path) -> Result<(), TgsError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

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
                token: "secret-token".to_owned(),
                chat_id: Some("@alerts".to_owned()),
            },
        );
        config.save(&path).expect("save");
        let loaded = Config::load(&path).expect("load");
        assert_eq!(loaded.default_profile.as_deref(), Some("ops"));
        assert_eq!(loaded.profiles["ops"].token, "secret-token");
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
}
