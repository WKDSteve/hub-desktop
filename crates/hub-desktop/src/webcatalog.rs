//! The catalog as the web hub (`hub.html`) embeds it: the same table, without anything a machine tells.
//!
//! The web hub cannot look at a machine (concept 5.4), so it needs the entries and the places of the web
//! variants, nothing else. The one source stays `catalog.rs`; `ui/src/web/catalog.json` is its rendering,
//! and a test holds the file to it (`UPDATE_WEB_CATALOG=1 cargo test -p hub-desktop web_catalog` writes it).

use serde::Serialize;

use crate::catalog::{Entry, CATALOG};
use crate::detect::{install_view, repo_view, status_word, InstallView, RepoView};

/// The place of the web variant of an app: a folder next to the others, and the page in it.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct WebPage {
    pub dir: &'static str,
    pub entry: &'static str,
}

/// One app as the web hub gets it.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct WebEntry {
    pub id: &'static str,
    pub name: &'static str,
    pub summary: &'static str,
    pub status: &'static str,
    /// A program exists: the web hub can only say that it cannot start it.
    pub native: bool,
    pub web: Option<WebPage>,
    pub install: InstallView,
    pub repo: RepoView,
}

fn entry(e: &'static Entry) -> WebEntry {
    WebEntry {
        id: e.id,
        name: e.name,
        summary: e.summary,
        status: status_word(e.status),
        native: e.native.is_some(),
        web: e.web.map(|w| WebPage {
            dir: w.repo_dir,
            entry: w.entry,
        }),
        install: install_view(e),
        repo: repo_view(e),
    }
}

/// Every app of the catalog, in the order they are shown.
#[must_use]
pub fn web_catalog() -> Vec<WebEntry> {
    CATALOG.iter().map(entry).collect()
}

/// The text of `ui/src/web/catalog.json`: pretty JSON with a final newline.
///
/// # Panics
/// Never in practice: the entries are plain strings and booleans.
#[must_use]
pub fn web_catalog_json() -> String {
    let mut text = serde_json::to_string_pretty(&web_catalog()).unwrap_or_default();
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn file() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../ui/src/web/catalog.json")
    }

    #[test]
    fn web_catalog_file_is_the_rendering_of_the_catalog() {
        let want = web_catalog_json();
        if std::env::var_os("UPDATE_WEB_CATALOG").is_some() {
            std::fs::write(file(), &want).expect("write catalog.json");
        }
        let have = std::fs::read_to_string(file()).unwrap_or_default();
        assert_eq!(
            have.replace("\r\n", "\n"),
            want,
            "ui/src/web/catalog.json is out of date: UPDATE_WEB_CATALOG=1 cargo test -p hub-desktop web_catalog"
        );
    }

    #[test]
    fn nothing_of_a_machine_is_in_it() {
        let text = web_catalog_json();
        for word in [
            "LOCALAPPDATA",
            "PROGRAMFILES",
            "vault_var",
            "port_var",
            "candidates",
        ] {
            assert!(!text.contains(word), "{word}");
        }
    }
}
