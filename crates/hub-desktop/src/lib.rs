//! The central hub: it lists the other apps and starts them with one click.
//!
//! The hub keeps no data of its own and runs no engine: it is the `appkit-hub` server with a runtime
//! that holds only its status, the routes below `/api/hub/` and the page. Each app it starts stays a
//! program of its own with its own hub, port, pairing and release.

pub mod app;
pub mod background;
pub mod catalog;
pub mod config;
pub mod detect;
pub mod launch;
pub mod paths;
pub mod prefs;
pub mod probe;
pub mod server;
pub mod ui;
pub mod webcatalog;
pub mod webserve;

pub use appkit_hub::client;

/// The names this app gives itself: config folder, environment variables, login entries, program
/// file, port and route namespace all come from here. The program is called like the repository,
/// because `hub-hub` (the pattern of the other apps) would be a poor name.
pub const IDENTITY: appkit_core::identity::AppIdentity = appkit_core::identity::AppIdentity {
    app: "hub-desktop",
    binary: "hub-desktop",
    env_prefix: "HUB_DESKTOP",
    title: "Hub",
    default_port: 17610,
    route_ns: "hub",
};

/// What the login entries and the launcher say about this app.
pub const LOGIN: appkit_hub::loginitems::LoginApp<'static> = appkit_hub::loginitems::LoginApp {
    identity: &IDENTITY,
    summary: "the start page of the other apps",
};

/// Version of this hub, as reported by `hello`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// What a build that is not a release says about itself (the commit it was made from): set by the
/// workflow of the rolling build through `HUB_DESKTOP_BUILD`, absent in a release and in a local
/// build.
pub const BUILD: Option<&str> = option_env!("HUB_DESKTOP_BUILD");

/// The line `--version` prints: `hub-desktop 0.0.0`, and `hub-desktop 0.0.0 (build abc1234)` for a
/// rolling build. A release is exactly the first form (the release workflow compares it with the tag).
#[must_use]
pub fn version_line() -> String {
    version_line_for(BUILD)
}

/// [`version_line`] for a given build (so that both forms are testable: `BUILD` is fixed at compile
/// time).
#[must_use]
pub fn version_line_for(build: Option<&str>) -> String {
    match build {
        Some(build) if !build.is_empty() => format!("hub-desktop {VERSION} (build {build})"),
        _ => format!("hub-desktop {VERSION}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{version_line_for, IDENTITY, VERSION};

    /// The names are on disk and in people's environments (the config folder, the `Run` value,
    /// `HUB_DESKTOP_*`): a rename is a migration, not a refactor.
    #[test]
    fn the_names_on_disk_and_in_the_environment_do_not_move() {
        assert_eq!(IDENTITY.app, "hub-desktop");
        assert_eq!(IDENTITY.binary, "hub-desktop");
        assert_eq!(IDENTITY.title, "Hub");
        assert_eq!(IDENTITY.default_port, 17610);
        assert_eq!(IDENTITY.route("hello"), "/api/hub/hello");
        assert_eq!(IDENTITY.var("CONFIG_DIR"), "HUB_DESKTOP_CONFIG_DIR");
        assert_eq!(IDENTITY.var("INSTALL_DIR"), "HUB_DESKTOP_INSTALL_DIR");
        assert_eq!(IDENTITY.var("NO_LAUNCH"), "HUB_DESKTOP_NO_LAUNCH");
        assert_eq!(IDENTITY.desktop_file(), "hub-desktop.desktop");
        let program = if cfg!(windows) {
            "hub-desktop.exe"
        } else {
            "hub-desktop"
        };
        assert_eq!(IDENTITY.program_file(), program);
    }

    #[test]
    fn the_version_line_names_the_build_only_when_there_is_one() {
        assert_eq!(version_line_for(None), format!("hub-desktop {VERSION}"));
        assert_eq!(version_line_for(Some("")), format!("hub-desktop {VERSION}"));
        assert_eq!(
            version_line_for(Some("abc1234")),
            format!("hub-desktop {VERSION} (build abc1234)")
        );
    }
}
