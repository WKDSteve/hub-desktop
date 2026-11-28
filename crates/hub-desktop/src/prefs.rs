//! What a person chose for a tile: the mode a click opens (`config.json`, `tiles.<id>.mode`), and whether the
//! app starts with the login (the switch of the tile).
//!
//! For the mode the hub writes only a word of two (`native`, `web`) for an app of the catalog that has that
//! mode. The file is read again before every write, so that what a person changed by hand in the meantime is
//! kept. The start with the login is the one state of the kit (`appkit_hub::loginstate`), which the app
//! switches the same way; the program passes its writer, the tests one of their own.
//!
//! The settings of the CI watcher (`ci_watch`, `ci_watch_interval_min`) are read and written here as well: the page
//! changes them, the watcher reads them before every round.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use appkit_core::identity::AppIdentity;

use crate::catalog::{self, Mode};
use crate::ciwatch::{self, Schedule};
use crate::config::{self, TileSettings};
use crate::detect::Detector;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrefsError {
    UnknownApp,
    /// The app has no such mode.
    NotAvailable(&'static str),
    /// A value the setting does not take (a fixed sentence).
    Invalid(&'static str),
    Failed(String),
}

/// Switches the start with the login of an app on (`true`) or off. The program passes the kit's
/// `loginstate::set`; without one (the tests that do not ask for it) the tiles have no switch.
pub type AutostartWriter =
    Arc<dyn Fn(&'static AppIdentity, bool) -> Result<(), String> + Send + Sync>;

pub struct Prefs {
    /// The config folder; none: the choice is held in memory only (tests).
    dir: Option<PathBuf>,
    detector: Arc<Detector>,
    login: Option<AutostartWriter>,
    write: Mutex<()>,
    /// The settings of the CI watcher without a config folder (tests); with one, the file holds them.
    ci: Mutex<Schedule>,
    /// The schedule last read from the file: a file that cannot be read keeps it.
    ci_read: Mutex<Schedule>,
}

impl Prefs {
    #[must_use]
    pub fn new(dir: Option<PathBuf>, detector: Arc<Detector>) -> Self {
        Self {
            dir,
            detector,
            login: None,
            write: Mutex::new(()),
            ci: Mutex::new(Schedule::default()),
            ci_read: Mutex::new(Schedule::default()),
        }
    }

    /// How the start with the login of an app is switched.
    #[must_use]
    pub fn with_login(
        mut self,
        writer: impl Fn(&'static AppIdentity, bool) -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        self.login = Some(Arc::new(writer));
        self
    }

    /// Switch the start with the login of the app `id` on or off: an app of the kit only (its own entry, or
    /// `start_with_hub` while hub-desktop starts with the login). The tiles are looked at again afterwards.
    ///
    /// # Errors
    ///
    /// [`PrefsError`]: not an app, not one of the kit, no writer, or what the system said.
    pub fn set_autostart(&self, id: &str, on: bool) -> Result<(), PrefsError> {
        let entry = catalog::find(id).ok_or(PrefsError::UnknownApp)?;
        let identity = entry
            .kit_identity()
            .ok_or(PrefsError::NotAvailable("autostart"))?;
        let writer = self
            .login
            .as_ref()
            .ok_or(PrefsError::NotAvailable("autostart"))?;
        // The list is in the settings of this hub: one writer at a time, as for the mode.
        let _guard = self
            .write
            .lock()
            .map_err(|_| PrefsError::Failed("the settings are locked".to_owned()))?;
        let done = writer(identity, on).map_err(PrefsError::Failed);
        self.detector.invalidate();
        done
    }

    /// The settings of the CI watcher as `config.json` holds them now, and a note when its interval is outside the
    /// bounds (then it is the default). A file that cannot be read keeps the last schedule, with a note.
    #[must_use]
    pub fn ci_schedule(&self) -> (Schedule, Option<String>) {
        let Some(dir) = &self.dir else {
            return (self.ci.lock().map(|s| *s).unwrap_or_default(), None);
        };
        match config::load_file(dir) {
            Ok(file) => {
                let (schedule, note) =
                    ciwatch::schedule_of(file.app.ci_watch, file.app.ci_watch_interval_min);
                if let Ok(mut kept) = self.ci_read.lock() {
                    *kept = schedule;
                }
                (schedule, note)
            }
            Err(e) => (
                self.ci_read.lock().map(|s| *s).unwrap_or_default(),
                Some(format!(
                    "config.json cannot be read ({e}); the CI watcher keeps its last settings"
                )),
            ),
        }
    }

    /// Whether a runner that a push waits for starts without asking (`runners.start_when_waiting`); a file that
    /// cannot be read says no.
    #[must_use]
    pub fn runner_auto(&self) -> bool {
        self.dir.as_ref().is_some_and(|dir| {
            config::load_file(dir).is_ok_and(|file| file.app.runners.start_when_waiting)
        })
    }

    /// Switch the CI watcher on or off and set its interval (1 to 60 minutes), in `config.json`. The default
    /// interval is written as no key; the switch on as no key either (on is the default).
    ///
    /// # Errors
    ///
    /// [`PrefsError::Invalid`]: the interval is outside the bounds; [`PrefsError::Failed`]: the file.
    pub fn set_ci(&self, schedule: Schedule) -> Result<(), PrefsError> {
        if !ciwatch::interval_ok(schedule.interval_min) {
            return Err(PrefsError::Invalid("interval_min is 1 to 60"));
        }
        let _guard = self
            .write
            .lock()
            .map_err(|_| PrefsError::Failed("the settings are locked".to_owned()))?;
        if let Some(dir) = &self.dir {
            config::update_file(dir, |file| {
                file.app.ci_watch = (!schedule.on).then_some(false);
                file.app.ci_watch_interval_min = (schedule.interval_min
                    != ciwatch::INTERVAL_DEFAULT_MIN)
                    .then_some(schedule.interval_min);
                Ok(())
            })
            .map_err(PrefsError::Failed)?;
        } else if let Ok(mut held) = self.ci.lock() {
            *held = schedule;
        }
        Ok(())
    }

    /// Choose (`Some`) or forget (`None`) the mode a click opens for `id`.
    ///
    /// # Errors
    ///
    /// [`PrefsError`]: nothing was changed.
    pub fn set_mode(&self, id: &str, mode: Option<Mode>) -> Result<(), PrefsError> {
        let entry = catalog::find(id).ok_or(PrefsError::UnknownApp)?;
        match mode {
            Some(Mode::Native) if entry.native.is_none() => {
                return Err(PrefsError::NotAvailable("native"));
            }
            Some(Mode::Web) if entry.web.is_none() => return Err(PrefsError::NotAvailable("web")),
            _ => {}
        }
        let _guard = self
            .write
            .lock()
            .map_err(|_| PrefsError::Failed("the settings are locked".to_owned()))?;
        if let Some(dir) = &self.dir {
            config::update_file(dir, |file| {
                match mode {
                    Some(mode) => {
                        file.app.tiles.insert(
                            entry.id.to_owned(),
                            TileSettings {
                                mode: Some(config::mode_word(mode).to_owned()),
                            },
                        );
                    }
                    None => {
                        file.app.tiles.remove(entry.id);
                    }
                }
                Ok(())
            })
            .map_err(PrefsError::Failed)?;
        }
        self.detector.set_mode(entry.id, mode);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use appkit_scratch::Scratch;

    use super::*;
    use crate::config::Env;

    fn prefs_in(dir: &std::path::Path) -> Prefs {
        let detector = Arc::new(Detector::new(
            Env::from_pairs([("APPDATA", "C:/none")]),
            None,
        ));
        Prefs::new(Some(dir.to_owned()), detector)
    }

    #[test]
    fn the_ci_settings_and_an_app_editing_the_list_lose_nothing_of_each_other() {
        let dir = Scratch::new("hub-prefs-ci-race");
        let path = dir.path().to_owned();
        let hub = path.clone();
        config::an_app_adds_to_the_list_while(&path, "tasks", move || {
            prefs_in(&hub)
                .set_ci(Schedule {
                    on: false,
                    interval_min: 7,
                })
                .expect("set_ci");
        });
        let file = config::load_file(&path).unwrap();
        assert_eq!(file.app.start_with_hub, ["tasks"]);
        assert_eq!(file.app.ci_watch, Some(false));
        assert_eq!(file.app.ci_watch_interval_min, Some(7));
    }

    #[test]
    fn the_mode_of_a_tile_and_an_app_editing_the_list_lose_nothing_of_each_other() {
        let dir = Scratch::new("hub-prefs-mode-race");
        let path = dir.path().to_owned();
        let hub = path.clone();
        config::an_app_adds_to_the_list_while(&path, "rules", move || {
            prefs_in(&hub)
                .set_mode("tasks", Some(Mode::Web))
                .expect("set_mode");
        });
        let file = config::load_file(&path).unwrap();
        assert_eq!(file.app.start_with_hub, ["rules"]);
        assert_eq!(
            file.app.tiles.get("tasks").and_then(|t| t.mode.as_deref()),
            Some("web")
        );
    }
}
