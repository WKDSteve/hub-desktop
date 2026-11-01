//! The settings of the hub: `config.json` in the config folder. The keys every app has come from
//! `appkit-hub`; the hub's own is `web_root`, the folder with the checkouts of the web variants. The
//! mode a tile opens in follows with the page.
//!
//! The file is read by the hub and written by a person. Nothing in it names a program or a way to
//! start one: those are in the catalog.

use std::collections::BTreeMap;
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
    /// The export folder of testing.nvim (`testing export --out`) the monitor reads; none: no monitor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub testing_export: Option<PathBuf>,
    /// What a person chose for a tile, by the id of the app of the catalog.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tiles: BTreeMap<String, TileSettings>,
}

/// The choices for one tile. Only the mode a click opens: nothing that names a program.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TileSettings {
    /// `native` or `web`; none: the default of the catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

/// The mode a word of the settings or of a request names.
#[must_use]
pub fn mode_of(word: &str) -> Option<crate::catalog::Mode> {
    match word {
        "native" => Some(crate::catalog::Mode::Native),
        "web" => Some(crate::catalog::Mode::Web),
        _ => None,
    }
}

/// The word of a mode.
#[must_use]
pub fn mode_word(mode: crate::catalog::Mode) -> &'static str {
    match mode {
        crate::catalog::Mode::Native => "native",
        crate::catalog::Mode::Web => "web",
    }
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
        if let Some(dir) = &self.testing_export
            && crate::paths::usable_root(dir.clone()).is_none()
        {
            return Err(format!(
                "testing_export: {} is not an absolute folder of this machine",
                dir.display()
            ));
        }
        for (id, tile) in &self.tiles {
            if crate::catalog::find(id).is_none() {
                return Err(format!("tiles: {id:?} is not an app of the catalog"));
            }
            if let Some(word) = &tile.mode
                && mode_of(word).is_none()
            {
                return Err(format!("tiles.{id}.mode: {word:?} is not native or web"));
            }
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

/// Write `config.json` into `dir` in one step.
///
/// # Errors
///
/// The reason, as text.
pub fn save_file(dir: &Path, cfg: &FileConfig) -> Result<(), String> {
    appkit_hub::config::save_file(dir, cfg)
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
                ..HubSettings::default()
            }
            .validate()
        };
        assert!(ok("C:/repos").is_ok());
        for bad in ["repos", r"\\server\share\w", "C:/a/../b"] {
            assert!(ok(bad).is_err(), "{bad}");
        }
        assert!(HubSettings::default().validate().is_ok());
    }

    #[test]
    fn the_export_folder_of_the_monitor_must_be_an_absolute_folder_of_this_machine() {
        let ok = |dir: &str| {
            HubSettings {
                testing_export: Some(PathBuf::from(dir)),
                ..HubSettings::default()
            }
            .validate()
        };
        assert!(ok("C:/results/export").is_ok());
        for bad in ["export", r"\\server\share\e", "C:/a/../b"] {
            assert!(ok(bad).is_err(), "{bad}");
        }
    }

    fn tiles(json: &str) -> Result<HubSettings, String> {
        let cfg: FileConfig = serde_json::from_str(json).map_err(|e| e.to_string())?;
        cfg.app.validate().map(|()| cfg.app)
    }

    #[test]
    fn a_tile_can_name_a_mode_of_an_app_of_the_catalog_and_nothing_else() {
        let ok = tiles(r#"{"tiles":{"tasks":{"mode":"web"},"rules":{}}}"#).unwrap();
        assert_eq!(ok.tiles["tasks"].mode.as_deref(), Some("web"));
        for bad in [
            r#"{"tiles":{"nope":{"mode":"web"}}}"#,
            r#"{"tiles":{"tasks":{"mode":"fly"}}}"#,
            r#"{"tiles":{"tasks":{"mode":"native","program":"C:/x.exe"}}}"#,
            r#"{"tiles":{"tasks":{"argv":["--evil"]}}}"#,
            r#"{"tiles":{"hub":{"mode":"native"}}}"#,
        ] {
            assert!(tiles(bad).is_err(), "{bad}");
        }
    }
}
