//! The catalog of the apps the hub knows: a constant table in the program (concept 4).
//!
//! Nothing here is read at run time from a place somebody can write to: the names, the ways to find an
//! app on the machine and the links to install it are part of the binary. A way to start an app is a
//! variant of [`Native`], never a command line; the program that a start runs comes from the detection
//! (`detect`), which only looks at the places named here.

use appkit_core::identity::AppIdentity;

/// Whether the repository of an app can be read by anybody.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Public,
    Private,
}

/// Where the code and the releases of an app are.
#[derive(Debug, Clone, Copy)]
pub struct Repo {
    pub owner: &'static str,
    pub name: &'static str,
    pub visibility: Visibility,
}

/// How far an app is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Can be installed and started.
    Available,
    /// Does not exist yet: shown below with a badge, no link.
    Planned,
    /// Has no installer: it is built from the source.
    BuildOnly,
}

/// How an app is opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Native,
    Web,
}

/// How a program of the machine is started and looked at. The argv of each way is in the code of the
/// start (`launch`), not here.
#[derive(Debug, Clone, Copy)]
pub enum Native {
    /// An app built on `appkit-hub`: its own hub (`<program> start --open`). Found at the install
    /// folder of that app (`<PREFIX>_INSTALL_DIR`, else `{LOCALAPPDATA}\<app>\bin\<binary>.exe`).
    AppKitHub {
        identity: &'static AppIdentity,
        /// The variable that names the vault of the app: without it and without a `vault` in its
        /// settings the app cannot be used yet.
        vault_var: &'static str,
    },
    /// A program with a window of its own and no hub: started with no argument.
    GuiExe { candidates: &'static [&'static str] },
    /// A program that serves a page on a loopback port: started in a console of its own, then the browser.
    LoopbackServer {
        candidates: &'static [&'static str],
        port_var: &'static str,
        default_port: u16,
    },
}

/// The web variant of an app: one HTML file in a checkout of its own, below the web folder.
#[derive(Debug, Clone, Copy)]
pub struct Web {
    /// The folder of the checkout (`tasks-web`).
    pub repo_dir: &'static str,
    /// The page in it (`tasks.html`).
    pub entry: &'static str,
}

/// What a person is told to do about an app that is not installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hint {
    /// Download the file from the release page and check it against `SHA256SUMS`.
    Release,
    /// Clone the repository of the web variant next to the others.
    Clone,
    /// Does not exist yet.
    Planned,
    /// Build it from the source.
    BuildYourself,
}

/// The way to get an app: always a link to a page and a file name, never a script.
#[derive(Debug, Clone, Copy)]
pub struct Install {
    pub hint: Hint,
    /// The page with the files (`https://github.com/…/releases/tag/latest`).
    pub page: Option<&'static str>,
    /// The file to download from it.
    pub file: Option<&'static str>,
}

/// One app.
#[derive(Debug, Clone, Copy)]
pub struct Entry {
    pub id: &'static str,
    pub name: &'static str,
    pub summary: &'static str,
    pub repo: Repo,
    pub status: Status,
    pub native: Option<Native>,
    pub web: Option<Web>,
    pub default_mode: Mode,
    pub install: Install,
}

/// The names of the two apps that are built on `appkit-hub`, as the apps give them to themselves. They
/// are on disk (the config folder, `hub.json`) and in the environment of the person: a rename on their
/// side is a migration, and `tests/catalog.rs` holds these copies to what they say.
pub const TASKS: AppIdentity = AppIdentity {
    app: "tasks-desktop",
    binary: "tasks-hub",
    env_prefix: "TASKS_DESKTOP",
    title: "Tasks",
    default_port: 17615,
    route_ns: "tasks",
};

pub const RULES: AppIdentity = AppIdentity {
    app: "rules-desktop",
    binary: "rules-hub",
    env_prefix: "RULES_DESKTOP",
    title: "Rules",
    default_port: 17616,
    route_ns: "rules",
};

/// The port of ai-desktop when `AI_DESKTOP_PORT` does not name one.
pub const AI_PORT: u16 = 8080;

const OWNER: &str = "WKDSteve";

/// The apps, in the order they are shown.
pub const CATALOG: &[Entry] = &[
    Entry {
        id: "tasks",
        name: "Tasks",
        summary: "The task overview of tasks.nvim",
        repo: Repo {
            owner: OWNER,
            name: "tasks-desktop",
            visibility: Visibility::Public,
        },
        status: Status::Available,
        native: Some(Native::AppKitHub {
            identity: &TASKS,
            vault_var: "TASKS_VAULT",
        }),
        web: Some(Web {
            repo_dir: "tasks-web",
            entry: "tasks.html",
        }),
        default_mode: Mode::Native,
        install: Install {
            hint: Hint::Release,
            page: Some("https://github.com/WKDSteve/tasks-desktop/releases/tag/latest"),
            file: Some("tasks-hub-windows-x86_64.exe"),
        },
    },
    Entry {
        id: "rules",
        name: "Rules",
        summary: "The rule sets of rules.nvim",
        repo: Repo {
            owner: OWNER,
            name: "rules-desktop",
            visibility: Visibility::Public,
        },
        status: Status::Available,
        native: Some(Native::AppKitHub {
            identity: &RULES,
            vault_var: "RULES_VAULT",
        }),
        web: None,
        default_mode: Mode::Native,
        install: Install {
            hint: Hint::Release,
            page: Some("https://github.com/WKDSteve/rules-desktop/releases/tag/latest"),
            file: Some("rules-hub-windows-x86_64.exe"),
        },
    },
    Entry {
        id: "docmap",
        name: "Docmap",
        summary: "The map of the documents of a project",
        repo: Repo {
            owner: OWNER,
            name: "docmap-desktop",
            visibility: Visibility::Private,
        },
        status: Status::Available,
        native: Some(Native::GuiExe {
            candidates: &[
                r"{LOCALAPPDATA}\docmap-desktop\docmap-desktop.exe",
                r"{PROGRAMFILES}\docmap-desktop\docmap-desktop.exe",
            ],
        }),
        web: None,
        default_mode: Mode::Native,
        install: Install {
            hint: Hint::Release,
            page: Some("https://github.com/WKDSteve/docmap-desktop/releases/latest"),
            file: Some("docmap-desktop-windows-x86_64-setup.exe"),
        },
    },
    Entry {
        id: "ai",
        name: "AI Desktop",
        summary: "The local AI workbench with its dashboard",
        repo: Repo {
            owner: OWNER,
            name: "ai-desktop",
            visibility: Visibility::Private,
        },
        status: Status::Available,
        native: Some(Native::LoopbackServer {
            candidates: &[
                r"{LOCALAPPDATA}\Programs\ai-desktop\app\ai-desktop.exe",
                r"{LOCALAPPDATA}\ai-desktop\app\ai-desktop.exe",
            ],
            port_var: "AI_DESKTOP_PORT",
            default_port: AI_PORT,
        }),
        web: None,
        default_mode: Mode::Native,
        install: Install {
            hint: Hint::Release,
            page: Some("https://github.com/WKDSteve/ai-desktop/releases/latest"),
            file: Some("ai-desktop-setup-x64.exe"),
        },
    },
    Entry {
        id: "testing",
        name: "Testing",
        summary: "The test runs of testing.nvim",
        repo: Repo {
            owner: OWNER,
            name: "testing-web",
            visibility: Visibility::Public,
        },
        status: Status::Planned,
        native: None,
        web: Some(Web {
            repo_dir: "testing-web",
            entry: "testing.html",
        }),
        default_mode: Mode::Web,
        install: Install {
            hint: Hint::Planned,
            page: None,
            file: None,
        },
    },
    Entry {
        id: "streamtap",
        name: "streamtap",
        summary: "Taps a stream into a file",
        repo: Repo {
            owner: OWNER,
            name: "streamtap",
            visibility: Visibility::Private,
        },
        status: Status::BuildOnly,
        native: None,
        web: None,
        default_mode: Mode::Native,
        install: Install {
            hint: Hint::BuildYourself,
            page: None,
            file: None,
        },
    },
];

/// The entry with this id.
#[must_use]
pub fn find(id: &str) -> Option<&'static Entry> {
    CATALOG.iter().find(|e| e.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ids_are_unique_plain_words() {
        for (i, entry) in CATALOG.iter().enumerate() {
            assert!(
                !entry.id.is_empty() && entry.id.bytes().all(|b| b.is_ascii_lowercase()),
                "{}",
                entry.id
            );
            assert!(
                CATALOG[i + 1..].iter().all(|other| other.id != entry.id),
                "{} twice",
                entry.id
            );
        }
    }

    /// What is not installable has no way to start: a planned or build-only app that got a program path
    /// would be one the hub can run without anybody having decided it.
    #[test]
    fn only_an_available_app_can_have_a_program() {
        for entry in CATALOG {
            if entry.status != Status::Available {
                assert!(entry.native.is_none(), "{}", entry.id);
            }
        }
        assert!(find("streamtap").unwrap().native.is_none());
        assert!(find("testing").unwrap().native.is_none());
    }

    #[test]
    fn a_default_mode_is_one_the_app_has() {
        for entry in CATALOG {
            match entry.default_mode {
                // An app with only a web variant would open natively into nothing.
                Mode::Native => assert!(
                    entry.native.is_some() || entry.web.is_none(),
                    "{}",
                    entry.id
                ),
                Mode::Web => assert!(entry.web.is_some(), "{}", entry.id),
            }
        }
    }

    #[test]
    fn a_link_to_install_is_https_to_the_repository_of_the_app_and_a_file_is_a_plain_name() {
        for entry in CATALOG {
            if let Some(page) = entry.install.page {
                let prefix = format!(
                    "https://github.com/{}/{}/releases/",
                    entry.repo.owner, entry.repo.name
                );
                assert!(page.starts_with(&prefix), "{page}");
            }
            if let Some(file) = entry.install.file {
                assert!(
                    file.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_')),
                    "{file}"
                );
                assert!(
                    !file.ends_with(".ps1") && !file.ends_with(".sh"),
                    "no script: {file}"
                );
            }
        }
    }

    #[test]
    fn the_hub_itself_is_not_in_its_catalog() {
        assert!(CATALOG.iter().all(|e| e.id != "hub"));
    }
}
