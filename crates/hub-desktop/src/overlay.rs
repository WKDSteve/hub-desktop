//! The overlay: a file of the person that hides tiles, orders them and names the folder of the web
//! variants (concept 4.1, 8). Schema `hub-catalog/1`.
//!
//! It is the one file of the hub that is read as a document of its own, so it has a version. The rules
//! that keep it harmless:
//!
//! - a higher major version is refused (the file says more than this hub understands);
//! - a key nobody knows is ignored (a later minor version may add some);
//! - what it can do is hide, order and `web_root`: it cannot add an app, and a program, a start or an
//!   argument is a key like any other unknown one and changes nothing;
//! - an id that is not in the catalog is ignored.
//!
//! A file that cannot be used never stops the hub: it runs without the overlay and says why.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::catalog;

/// The file in the config folder.
pub const FILE: &str = "catalog-overlay.json";

/// The schema this hub reads: `hub-catalog/<major>`, with anything after the major (`.2`) ignored.
pub const SCHEMA: &str = "hub-catalog";
/// The highest major version of the schema this hub understands.
pub const MAJOR: u64 = 1;

/// A file larger than this is no overlay.
const MAX_BYTES: u64 = 64 * 1024;

/// What the overlay says, reduced to what it may say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overlay {
    /// Ids of apps of the catalog that are not shown.
    hidden: BTreeSet<&'static str>,
    /// Ids of apps of the catalog, in the order they are shown first.
    order: Vec<&'static str>,
    /// The folder with the checkouts of the web variants.
    pub web_root: Option<PathBuf>,
}

/// Why a file is not used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayError {
    /// Not JSON, not an object, or no `schema` of the form `hub-catalog/<major>`.
    Invalid(String),
    /// The major version is higher than [`MAJOR`].
    TooNew(u64),
    /// The file is larger than the hub reads.
    TooLarge,
    Unreadable(String),
}

impl std::fmt::Display for OverlayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(why) => write!(f, "{FILE} is not usable: {why}"),
            Self::TooNew(major) => write!(
                f,
                "{FILE} is of {SCHEMA}/{major}; this hub reads up to {SCHEMA}/{MAJOR}"
            ),
            Self::TooLarge => write!(f, "{FILE} is larger than {MAX_BYTES} bytes"),
            Self::Unreadable(why) => write!(f, "{FILE} cannot be read: {why}"),
        }
    }
}

/// The major version in `hub-catalog/<major>[.<minor>...]`.
fn major_of(schema: &str) -> Option<u64> {
    let rest = schema.strip_prefix(SCHEMA)?.strip_prefix('/')?;
    let digits = rest.split('.').next()?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// The ids of a list of strings that are in the catalog, once each, in the order given. Anything
/// else in the list (a number, an unknown id) is skipped.
fn ids(value: Option<&Value>) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for word in value.and_then(Value::as_array).into_iter().flatten() {
        if let Some(entry) = word.as_str().and_then(catalog::find)
            && !out.contains(&entry.id)
        {
            out.push(entry.id);
        }
    }
    out
}

impl Overlay {
    /// Read the text of an overlay.
    ///
    /// # Errors
    ///
    /// [`OverlayError`] when the text is not a document of a schema this hub reads.
    pub fn parse(text: &str) -> Result<Self, OverlayError> {
        let doc: Value =
            serde_json::from_str(text).map_err(|e| OverlayError::Invalid(e.to_string()))?;
        let Some(object) = doc.as_object() else {
            return Err(OverlayError::Invalid("not an object".to_owned()));
        };
        let schema = object
            .get("schema")
            .and_then(Value::as_str)
            .ok_or_else(|| OverlayError::Invalid("no schema".to_owned()))?;
        let major = major_of(schema).ok_or_else(|| {
            OverlayError::Invalid(format!("schema {schema:?} is not {SCHEMA}/<n>"))
        })?;
        if major > MAJOR {
            return Err(OverlayError::TooNew(major));
        }
        // A folder that is not a usable one is no folder: the overlay is ignored in that key, not refused.
        let web_root = object
            .get("web_root")
            .and_then(Value::as_str)
            .and_then(|s| crate::paths::usable_root(PathBuf::from(s)));
        Ok(Self {
            hidden: ids(object.get("hidden")).into_iter().collect(),
            order: ids(object.get("order")),
            web_root,
        })
    }

    /// Read the overlay of a config folder. A missing file is an empty overlay.
    ///
    /// # Errors
    ///
    /// [`OverlayError`] when the file exists and cannot be used.
    pub fn load(dir: &Path) -> Result<Self, OverlayError> {
        let path = dir.join(FILE);
        let meta = match std::fs::metadata(&path) {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(OverlayError::Unreadable(e.to_string())),
        };
        if meta.len() > MAX_BYTES {
            return Err(OverlayError::TooLarge);
        }
        let text =
            std::fs::read_to_string(&path).map_err(|e| OverlayError::Unreadable(e.to_string()))?;
        Self::parse(&text)
    }

    /// Whether the person hid the app.
    #[must_use]
    pub fn hides(&self, id: &str) -> bool {
        self.hidden.contains(id)
    }

    /// The position of an app in the order the person gave; apps without one keep the order of the
    /// catalog behind those that have one.
    #[must_use]
    pub fn rank(&self, id: &str) -> usize {
        self.order
            .iter()
            .position(|o| *o == id)
            .unwrap_or(usize::MAX)
    }
}

#[cfg(test)]
mod tests {
    use appkit_scratch::Scratch;

    use super::*;

    fn parse(text: &str) -> Result<Overlay, OverlayError> {
        Overlay::parse(text)
    }

    #[test]
    fn hides_orders_and_names_a_folder() {
        let o = parse(
            r#"{"schema":"hub-catalog/1","hidden":["streamtap","ai"],"order":["rules","tasks"],"web_root":"C:/repos"}"#,
        )
        .unwrap();
        assert!(o.hides("streamtap") && o.hides("ai") && !o.hides("tasks"));
        assert!(o.rank("rules") < o.rank("tasks"));
        assert_eq!(o.rank("docmap"), usize::MAX);
        assert_eq!(o.web_root, Some(PathBuf::from("C:/repos")));
    }

    #[test]
    fn a_higher_major_is_refused_and_a_minor_is_not() {
        assert_eq!(
            parse(r#"{"schema":"hub-catalog/2"}"#),
            Err(OverlayError::TooNew(2))
        );
        assert_eq!(
            parse(r#"{"schema":"hub-catalog/10.1"}"#),
            Err(OverlayError::TooNew(10))
        );
        assert!(parse(r#"{"schema":"hub-catalog/1.7","hidden":["ai"]}"#)
            .unwrap()
            .hides("ai"));
    }

    #[test]
    fn a_file_without_a_schema_of_this_kind_is_refused() {
        for bad in [
            "{}",
            "[]",
            "not json",
            r#"{"schema":1}"#,
            r#"{"schema":"hub-catalog"}"#,
            r#"{"schema":"hub-catalog/"}"#,
            r#"{"schema":"hub-catalog/x"}"#,
            r#"{"schema":"other/1"}"#,
            r#"{"schema":"hub-catalog/-1"}"#,
        ] {
            assert!(matches!(parse(bad), Err(OverlayError::Invalid(_))), "{bad}");
        }
    }

    #[test]
    fn what_nobody_knows_is_ignored() {
        let o = parse(
            r#"{"schema":"hub-catalog/1","future":{"a":1},"hidden":["nope",7,"ai","ai"],"order":["nope"]}"#,
        )
        .unwrap();
        assert!(o.hides("ai") && !o.hides("nope"));
        assert_eq!(o.rank("nope"), usize::MAX);
        assert_eq!(o.hidden.len(), 1);
    }

    /// The overlay cannot bring a program or a way to start one, and cannot add an app: such keys are
    /// unknown keys and the result is the empty overlay.
    #[test]
    fn it_cannot_name_a_program_or_add_an_app() {
        let o = parse(
            r#"{"schema":"hub-catalog/1","program":"C:/Windows/System32/cmd.exe","argv":["/c","calc"],"native":{"exe":"x.exe"},"apps":[{"id":"evil","name":"Evil"}],"tiles":{"tasks":{"mode":"native","program":"x"}}}"#,
        )
        .unwrap();
        assert_eq!(o, Overlay::default());
    }

    #[test]
    fn a_folder_that_is_not_usable_is_dropped_not_refused() {
        for bad in ["repos", r"\\server\share\w", "C:/a/../b", ""] {
            let text =
                format!(r#"{{"schema":"hub-catalog/1","hidden":["ai"],"web_root":{bad:?}}}"#);
            let o = parse(&text).unwrap();
            assert!(o.web_root.is_none() && o.hides("ai"), "{bad}");
        }
        assert!(parse(r#"{"schema":"hub-catalog/1","web_root":5}"#)
            .unwrap()
            .web_root
            .is_none());
    }

    #[test]
    fn a_missing_file_is_empty_and_a_large_or_bad_one_is_an_error() {
        let dir = Scratch::new("hub-overlay");
        assert_eq!(Overlay::load(dir.path()), Ok(Overlay::default()));
        std::fs::write(
            dir.path().join(FILE),
            r#"{"schema":"hub-catalog/1","hidden":["ai"]}"#,
        )
        .unwrap();
        assert!(Overlay::load(dir.path()).unwrap().hides("ai"));
        std::fs::write(dir.path().join(FILE), vec![b' '; 70_000]).unwrap();
        assert_eq!(Overlay::load(dir.path()), Err(OverlayError::TooLarge));
        std::fs::write(dir.path().join(FILE), "{").unwrap();
        assert!(matches!(
            Overlay::load(dir.path()),
            Err(OverlayError::Invalid(_))
        ));
    }
}
