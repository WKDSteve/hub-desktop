//! The settings of the hub: `config.json` in the config folder. The keys every app has come from
//! `appkit-hub`; the hub's own is `web_root`, the folder with the checkouts of the web variants. The
//! mode a tile opens in follows with the page.
//!
//! The file is read by the hub and written by a person. Nothing in it names a program or a way to
//! start one: those are in the catalog.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use appkit_hub::config::{AppSettings, ConfigError};
pub use appkit_hub::env::Env;

/// The keys of `config.json` that only the hub has.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HubSettings {
    /// The folder that holds the checkouts of the web variants (`tasks-web`, …); none: `$REPOS_DIR`, then
    /// `<home>\hub-web-apps`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web_root: Option<PathBuf>,
}

impl AppSettings for HubSettings {
    /// A folder that is not absolute would mean whatever the working folder is; a share or a device
    /// path is not a place the hub looks for programs or pages.
    fn validate(&self) -> Result<(), String> {
        if let Some(root) = &self.web_root
            && crate::paths::usable_root(root.clone()).is_none()
        {
            return Err(format!(
                "web_root: {} is not an absolute folder of this machine",
                root.display()
            ));
        }
        Ok(())
    }
}

/// The settings file of the hub.
pub type FileConfig = appkit_hub::config::FileConfig<HubSettings>;

/// The config folder of this app on this device (see [`appkit_hub::config::config_dir`]).
#[must_use]
pub fn config_dir(env: &Env) -> Option<PathBuf> {
    appkit_hub::config::config_dir(env, &crate::IDENTITY)
}

/// Load `config.json` from `dir`. A missing file is an empty config.
///
/// # Errors
///
/// [`ConfigError`] when the file exists and cannot be read or is not valid.
pub fn load_file(dir: &Path) -> Result<FileConfig, ConfigError> {
    appkit_hub::config::load_file(dir)
}

#[cfg(test)]
mod tests {
    use appkit_scratch::Scratch;

    use super::*;

    #[test]
    fn env_override_wins_for_the_config_dir() {
        let env = Env::from_pairs([("HUB_DESKTOP_CONFIG_DIR", "C:/x/cfg"), ("APPDATA", "C:/a")]);
        assert_eq!(config_dir(&env), Some(PathBuf::from("C:/x/cfg")));
    }

    #[test]
    fn a_missing_file_is_an_empty_config() {
        let parent = Scratch::new("hub-no-such-config-dir");
        let dir = parent.path().join("absent");
        let cfg = load_file(&dir).expect("a missing file is fine");
        assert!(cfg.port.is_none() && cfg.app.web_root.is_none());
    }

    #[test]
    fn a_key_nobody_knows_is_refused() {
        assert!(serde_json::from_str::<FileConfig>(r#"{"port":17610}"#).is_ok());
        assert!(serde_json::from_str::<FileConfig>(r#"{"prot":17610}"#).is_err());
        assert!(serde_json::from_str::<FileConfig>(r#"{"web_root":"C:/w","port":17610}"#).is_ok());
    }

    /// The file can name where the pages are and nothing that is run: a key for a program is a key
    /// nobody knows.
    #[test]
    fn the_file_cannot_name_a_program_or_a_way_to_start_one() {
        for key in [
            "program", "command", "argv", "exe", "launch", "native", "args", "path",
        ] {
            let text = format!(r#"{{"{key}":"C:/Windows/System32/cmd.exe"}}"#);
            assert!(serde_json::from_str::<FileConfig>(&text).is_err(), "{key}");
        }
    }

    #[test]
    fn the_web_root_must_be_an_absolute_folder_of_this_machine() {
        let ok = |root: &str| {
            HubSettings {
                web_root: Some(PathBuf::from(root)),
            }
            .validate()
        };
        assert!(ok("C:/repos").is_ok());
        for bad in ["repos", r"\\server\share\w", "C:/a/../b"] {
            assert!(ok(bad).is_err(), "{bad}");
        }
        assert!(HubSettings::default().validate().is_ok());
    }
}
