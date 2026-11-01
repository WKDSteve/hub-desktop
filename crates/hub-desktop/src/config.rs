//! The settings of the hub: `config.json` in the config folder. The keys every app has come from
//! `appkit-hub`; the hub has no key of its own yet (the folder of the web variants and the mode a
//! tile opens in follow with the catalog).

use std::path::{Path, PathBuf};

pub use appkit_hub::config::{AppSettings, ConfigError, NoSettings};
pub use appkit_hub::env::Env;

/// The settings file of the hub.
pub type FileConfig = appkit_hub::config::FileConfig<NoSettings>;

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
        assert!(cfg.port.is_none() && cfg.vault.is_none());
    }

    #[test]
    fn a_key_nobody_knows_is_refused() {
        assert!(serde_json::from_str::<FileConfig>(r#"{"port":17610}"#).is_ok());
        assert!(serde_json::from_str::<FileConfig>(r#"{"prot":17610}"#).is_err());
    }
}
