//! What a person chose for a tile: the mode a click opens (`config.json`, `tiles.<id>.mode`).
//!
//! The only thing the hub writes to its settings, and it writes only a word of two (`native`, `web`) for an
//! app of the catalog that has that mode. The file is read again before every write, so that what a person
//! changed by hand in the meantime is kept.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::catalog::{self, Mode};
use crate::config::{self, TileSettings};
use crate::detect::Detector;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrefsError {
    UnknownApp,
    /// The app has no such mode.
    NotAvailable(&'static str),
    Failed(String),
}

pub struct Prefs {
    /// The config folder; none: the choice is held in memory only (tests).
    dir: Option<PathBuf>,
    detector: Arc<Detector>,
    write: Mutex<()>,
}

impl Prefs {
    #[must_use]
    pub fn new(dir: Option<PathBuf>, detector: Arc<Detector>) -> Self {
        Self {
            dir,
            detector,
            write: Mutex::new(()),
        }
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
            let mut file = config::load_file(dir).map_err(|e| PrefsError::Failed(e.to_string()))?;
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
            config::save_file(dir, &file).map_err(PrefsError::Failed)?;
        }
        self.detector.set_mode(entry.id, mode);
        Ok(())
    }
}
