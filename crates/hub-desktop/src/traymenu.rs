//! What the menu of the icon in the notification area shows and what its entries do, apart from the
//! icon itself (`tray.rs`, Windows), so that it can be tested everywhere.
//!
//! The menu: "Open hub", "CI: n hints" when the CI watcher has some (it opens the page, which names them), one
//! submenu per installed app of the kit (its state, "Open", "Stop"), "Quit hub". An entry is known by its id, a
//! word written here (`open-hub`, `ci-hints`, `open:<app>`, `stop:<app>`, `quit`); an id that is not one of these,
//! or that names no app of the catalog with a hub of the kit, does nothing.

use crate::catalog;
use crate::detect::{AppState, Run};

/// The id of "Open hub".
pub const OPEN_HUB: &str = "open-hub";
/// The id of "Quit hub".
pub const QUIT: &str = "quit";
/// The id of "CI: n hints" (it opens the page: the hints are in its section "Testing").
pub const CI_HINTS: &str = "ci-hints";

/// The line of the hints of the CI watcher; none when it has none.
#[must_use]
pub fn ci_label(hints: usize) -> Option<String> {
    match hints {
        0 => None,
        1 => Some("CI: 1 hint".to_owned()),
        n => Some(format!("CI: {n} hints")),
    }
}

/// One app of the kit in the menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HubLine {
    pub id: &'static str,
    pub name: &'static str,
    pub state: HubState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubState {
    Running,
    Stopped,
    /// Something else answers on its port.
    PortBusy,
    /// It cannot be used yet (no vault): "Open" asks its questions on the page of the hub.
    Setup,
}

/// What an entry of the menu asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// The page of hub-desktop, paired, in the browser.
    OpenHub,
    /// The app as a click on its tile opens it (`<program> start --open`).
    Open(&'static str),
    /// Its hub stops (`<program> stop`).
    Stop(&'static str),
    /// hub-desktop ends; the other hubs stay.
    Quit,
}

/// The lines of the installed apps of the kit, from a look of the detection.
#[must_use]
pub fn lines(apps: &[AppState]) -> Vec<HubLine> {
    apps.iter()
        .filter(|a| a.entry.is_kit_hub() && a.native.is_some())
        .map(|a| HubLine {
            id: a.entry.id,
            name: a.entry.name,
            state: match (a.run, a.setup.is_some()) {
                (Some(Run::Running), _) => HubState::Running,
                (Some(Run::PortBusy), _) => HubState::PortBusy,
                (_, true) => HubState::Setup,
                _ => HubState::Stopped,
            },
        })
        .collect()
}

/// The text of the submenu of an app: its name and its state, in words (never a colour alone).
#[must_use]
pub fn label(line: &HubLine) -> String {
    let state = match line.state {
        HubState::Running => "running",
        HubState::Stopped => "not running",
        HubState::PortBusy => "port in use",
        HubState::Setup => "needs setup",
    };
    format!("{} \u{2014} {state}", line.name)
}

/// Whether "Stop" can be chosen for the app.
#[must_use]
pub fn can_stop(line: &HubLine) -> bool {
    line.state == HubState::Running
}

/// The id of "Open" for an app.
#[must_use]
pub fn open_id(app: &str) -> String {
    format!("open:{app}")
}

/// The id of "Stop" for an app.
#[must_use]
pub fn stop_id(app: &str) -> String {
    format!("stop:{app}")
}

/// What the entry with `id` asks for; `None`: nothing.
#[must_use]
pub fn action(id: &str) -> Option<Action> {
    let kit = |app: &str| catalog::find(app).filter(|e| e.is_kit_hub()).map(|e| e.id);
    match id {
        OPEN_HUB | CI_HINTS => Some(Action::OpenHub),
        QUIT => Some(Action::Quit),
        _ => {
            if let Some(app) = id.strip_prefix("open:") {
                kit(app).map(Action::Open)
            } else if let Some(app) = id.strip_prefix("stop:") {
                kit(app).map(Action::Stop)
            } else {
                None
            }
        }
    }
}

/// The text over the icon: how many of the hubs run.
#[must_use]
pub fn tooltip(lines: &[HubLine]) -> String {
    let running = lines
        .iter()
        .filter(|l| l.state == HubState::Running)
        .count();
    if lines.is_empty() {
        "Hub".to_owned()
    } else {
        format!("Hub \u{2014} {running} of {} hubs running", lines.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::find;

    fn state(id: &str, installed: bool, run: Option<Run>, setup: bool) -> AppState {
        AppState {
            entry: find(id).unwrap(),
            native: installed.then(|| std::path::PathBuf::from("C:/p.exe")),
            web: None,
            run,
            setup: setup.then(|| "no_vault".to_owned()),
            port: None,
            chosen: None,
            autostart: None,
            build: None,
        }
    }

    #[test]
    fn the_menu_lists_the_installed_hubs_of_the_kit_with_their_state_in_words() {
        let apps = [
            state("tasks", true, Some(Run::Running), false),
            state("rules", true, Some(Run::Stopped), true),
            state("testing", false, Some(Run::Stopped), false),
            state("docmap", true, None, false),
            state("pult", true, Some(Run::PortBusy), false),
        ];
        let got = lines(&apps);
        let ids: Vec<&str> = got.iter().map(|l| l.id).collect();
        assert_eq!(ids, ["tasks", "rules", "pult"]);
        assert_eq!(label(&got[0]), "Tasks \u{2014} running");
        assert_eq!(label(&got[1]), "Rules \u{2014} needs setup");
        assert_eq!(got[2].state, HubState::PortBusy);
        assert!(can_stop(&got[0]) && !can_stop(&got[1]) && !can_stop(&got[2]));
        assert_eq!(tooltip(&got), "Hub \u{2014} 1 of 3 hubs running");
        assert_eq!(tooltip(&[]), "Hub");
    }

    #[test]
    fn the_hints_of_the_ci_watcher_are_a_line_only_when_there_are_some() {
        assert_eq!(ci_label(0), None);
        assert_eq!(ci_label(1).as_deref(), Some("CI: 1 hint"));
        assert_eq!(ci_label(3).as_deref(), Some("CI: 3 hints"));
    }

    #[test]
    fn an_entry_does_what_its_id_says_for_a_hub_of_the_kit_only() {
        assert_eq!(action(OPEN_HUB), Some(Action::OpenHub));
        assert_eq!(action(QUIT), Some(Action::Quit));
        assert_eq!(action(CI_HINTS), Some(Action::OpenHub));
        assert_eq!(action(&open_id("tasks")), Some(Action::Open("tasks")));
        assert_eq!(action(&stop_id("rules")), Some(Action::Stop("rules")));
        for nothing in [
            "",
            "open:",
            "open:docmap",
            "stop:ai",
            "open:nope",
            "open:tasks ",
            "OPEN:tasks",
            "start:tasks",
            "open:../tasks",
        ] {
            assert_eq!(action(nothing), None, "{nothing:?}");
        }
    }
}
