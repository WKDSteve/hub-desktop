//! The files of the user interface, held in memory.
//!
//! The built UI (`ui/dist`) is embedded in the binary by `build.rs`. The files sit in a table keyed
//! by the name the page asks for. A request is answered from the table or not at all: no path is
//! ever joined and opened per request, so there is nothing to traverse.

use std::collections::HashMap;

use axum::body::Bytes;

include!(concat!(env!("OUT_DIR"), "/ui_embedded.rs"));

#[derive(Clone)]
pub struct UiFile {
    pub bytes: Bytes,
    pub content_type: &'static str,
}

#[derive(Clone, Default)]
pub struct UiFiles {
    files: HashMap<String, UiFile>,
}

impl UiFiles {
    /// No interface: the hub answers with the placeholder page of the kit.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// The interface that is part of this binary (empty when the binary was built without
    /// `ui/dist`).
    #[must_use]
    pub fn embedded() -> Self {
        let files = EMBEDDED
            .iter()
            .map(|(key, content_type, bytes)| {
                (
                    (*key).to_owned(),
                    UiFile {
                        bytes: Bytes::from_static(bytes),
                        content_type,
                    },
                )
            })
            .collect();
        Self { files }
    }

    /// Is there a page to serve?
    #[must_use]
    pub fn has_index(&self) -> bool {
        self.files.contains_key("index.html")
    }

    /// The file for a request path (`/`, `/index.html`, `/assets/x.js`).
    #[must_use]
    pub fn get(&self, request_path: &str) -> Option<&UiFile> {
        let key = match request_path {
            "/" => "index.html",
            other => other.strip_prefix('/')?,
        };
        self.files.get(key)
    }
}

impl appkit_hub::Pages for UiFiles {
    fn page(&self, request_path: &str) -> Option<appkit_hub::Page> {
        self.get(request_path).map(|file| appkit_hub::Page {
            bytes: file.bytes.clone(),
            content_type: file.content_type,
        })
    }

    fn has_index(&self) -> bool {
        UiFiles::has_index(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hub_without_an_interface_has_no_index() {
        let ui = UiFiles::none();
        assert!(!ui.has_index());
        assert!(ui.get("/").is_none());
        assert!(ui.get("/index.html").is_none());
    }

    /// Only names of the table are answered: a path that climbs, names a drive or is not rooted
    /// finds nothing.
    #[test]
    fn only_a_name_of_the_table_is_answered() {
        let ui = UiFiles::embedded();
        for path in [
            "/../Cargo.toml",
            "/assets/../index.html.bak",
            "index.html",
            "/C:/x",
            "//",
            "/nope.js",
        ] {
            assert!(ui.get(path).is_none(), "{path}");
        }
    }
}
