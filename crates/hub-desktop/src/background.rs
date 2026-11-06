//! The hub as a program on the machine (`start`, `stop`, `install`, `uninstall`): the work is the
//! kit's, this module states what is the hub's own. There is no `autostart`: the hub is started by
//! a person (a link, a double click), not with the login.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use appkit_hub::background::Background;
pub use appkit_hub::background::{run_command, start_from, InstallOptions, UninstallOptions};
use appkit_hub::background::{LinkName, LinkStart};

use crate::config::Env;

/// What the background commands know about this app. The hub keeps no file in the config folder
/// besides those every hub has.
pub(crate) const APP: Background<'static> = Background {
    login: crate::LOGIN,
    extra_files: &[],
    uninstall_script: "scripts/uninstall",
};

/// Start a hub in the background unless one runs. Returns what to print.
///
/// # Errors
///
/// A text for the person at the terminal.
pub async fn start() -> Result<String, String> {
    appkit_hub::background::start(&APP).await
}

/// Stop the hub that runs (the one `hub.json` names and that answers as it).
///
/// # Errors
///
/// A text for the person at the terminal.
pub async fn stop() -> Result<String, String> {
    appkit_hub::background::stop(&APP).await
}

/// Where `install` puts this program.
///
/// # Errors
///
/// The reason there is no install folder.
pub fn installed_program(env: &Env) -> Result<PathBuf, String> {
    appkit_hub::background::installed_program(&APP, env)
}

/// What `install` is asked for: with a link, it is called `hub-desktop.lnk` and runs the hub in a minimized console,
/// which is the hub for as long as it stays open.
#[must_use]
pub fn install_options(shortcut: bool) -> InstallOptions {
    InstallOptions {
        shortcut,
        link_name: LinkName::App,
        link_start: LinkStart::ServeMinimized,
        ..InstallOptions::default()
    }
}

/// Copy this program to a fixed place and start the hub from there again when one was running.
///
/// # Errors
///
/// A text for the person at the terminal.
pub async fn install(opts: InstallOptions) -> Result<String, String> {
    appkit_hub::background::install(&APP, opts).await
}

/// Take the integration of the program with the system away.
pub fn uninstall(
    opts: UninstallOptions,
) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>> {
    appkit_hub::background::uninstall(&APP, opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_link_is_named_by_the_app_and_runs_the_hub_minimized() {
        let opts = super::install_options(true);
        assert!(opts.shortcut);
        assert_eq!(opts.link_name, super::LinkName::App);
        assert_eq!(opts.link_start, super::LinkStart::ServeMinimized);
        assert!(!super::install_options(false).shortcut);
    }

    /// `uninstall --purge-settings` removes the files of the kit and nothing else: the hub names
    /// no file of its own, so a file in the config folder that is not the kit's stays.
    #[test]
    fn the_hub_names_no_file_besides_the_kits() {
        assert!(APP.extra_files.is_empty());
        assert_eq!(APP.login.summary, "the start page of the other apps");
        assert_eq!(APP.uninstall_script, "scripts/uninstall");
        assert!(crate::IDENTITY.program_file().starts_with("hub-desktop"));
    }
}
