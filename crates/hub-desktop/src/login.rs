//! `hub-desktop autostart on | off | status`: hub-desktop starts with the login, and with it the hubs
//! of the apps in `start_with_hub` (`config.json`).
//!
//! So that there is one icon in the notification area, not one program per app, `on` takes the login
//! entries of the apps over: an app of [`TAKES_OVER`] whose own value is in the `Run` key is put into
//! `start_with_hub` and its value is removed, in that order (a failure leaves a hub started twice, which
//! its `start` answers with "already running", never none). `off` removes the value of hub-desktop and
//! gives each app of the list its own value back (the installed copy), so that what each app shows as
//! its start with the login stays as it was; the list is kept for the next `on`. The state an app and
//! its tile show is `appkit_hub::loginstate` (its own value, or the list while hub-desktop starts).
//!
//! The registry is reached through [`LoginEntries`]: the program passes [`SystemLogin`] (the kit's
//! `reg.exe` calls), the tests a fake, so that no test ever touches the login of the machine.

use std::path::{Path, PathBuf};

use appkit_core::identity::AppIdentity;
use appkit_hub::background::{Autostart, Background};
use appkit_hub::loginitems::LoginApp;

use crate::catalog;
use crate::config;

/// The apps whose own login entry `on` takes over: their value starts their hub (`<binary> start`). pult is
/// not one: its value starts its window, which has an icon of its own. The kit holds the list, so that the
/// apps switch their start with the login the same way (`appkit_hub::loginstate`).
pub const TAKES_OVER: [&str; 3] = appkit_hub::loginstate::TAKEN_OVER;

/// The login entries of the machine.
pub trait LoginEntries {
    /// `autostart on | off | status` of hub-desktop itself; returns what to print.
    ///
    /// # Errors
    ///
    /// A text for the person at the terminal.
    fn own(&self, what: Autostart) -> Result<String, String>;
    /// Whether another app has a value of its own; `None`: it cannot be told.
    fn of(&self, identity: &'static AppIdentity) -> Option<bool>;
    /// Remove the value of another app.
    ///
    /// # Errors
    ///
    /// A text for the person at the terminal.
    fn remove(&self, identity: &'static AppIdentity) -> Result<(), String>;
    /// Write the value of another app so that it starts `program`.
    ///
    /// # Errors
    ///
    /// A text for the person at the terminal.
    fn give(&self, identity: &'static AppIdentity, program: &Path) -> Result<(), String>;
    /// The program the value of another app names: its installed copy.
    ///
    /// # Errors
    ///
    /// The app is not installed.
    fn program(&self, identity: &'static AppIdentity) -> Result<PathBuf, String>;
}

/// The login entries of this machine, through the kit.
pub struct SystemLogin;

impl LoginEntries for SystemLogin {
    fn own(&self, what: Autostart) -> Result<String, String> {
        appkit_hub::background::autostart(&crate::background::APP, what)
    }

    fn of(&self, identity: &'static AppIdentity) -> Option<bool> {
        appkit_hub::background::autostart_of(identity)
    }

    fn remove(&self, identity: &'static AppIdentity) -> Result<(), String> {
        // The value is named by the app; the program path is not used to remove it.
        let other = Background {
            login: LoginApp {
                identity,
                summary: "",
            },
            extra_files: &[],
            uninstall_script: "",
        };
        appkit_hub::background::autostart_for(&other, Autostart::Off, Path::new("")).map(|_| ())
    }

    fn give(&self, identity: &'static AppIdentity, program: &Path) -> Result<(), String> {
        use appkit_hub::loginstate::LoginEntries as _;
        appkit_hub::loginstate::SystemLogin.set(identity, Some(program))
    }

    fn program(&self, identity: &'static AppIdentity) -> Result<PathBuf, String> {
        appkit_hub::loginstate::installed_program(identity, &crate::config::Env::from_process())
    }
}

/// `autostart on | off | status` with the settings in `dir`. Returns what to print.
///
/// # Errors
///
/// A text for the person at the terminal: the value of hub-desktop could not be written or removed, or
/// the settings could not be read or written.
pub fn autostart(dir: &Path, what: Autostart, login: &dyn LoginEntries) -> Result<String, String> {
    // Read first: a file that cannot be read changes nothing at the login.
    let mut file = config::load_file(dir).map_err(|e| e.to_string())?;
    let mut out = login.own(what)?;
    match what {
        Autostart::On => {
            let mut taken = Vec::new();
            for id in TAKES_OVER {
                let Some(identity) = catalog::find(id).and_then(catalog::Entry::kit_identity)
                else {
                    continue;
                };
                if login.of(identity) != Some(true) {
                    continue;
                }
                taken.push((id, identity));
            }
            if !taken.is_empty() {
                // The list first: a value that cannot be removed then starts a hub twice, never none.
                // Read again under the lock: an app may have changed the list since.
                config::update_file(dir, |now| {
                    for (id, _) in &taken {
                        if !now.app.start_with_hub.iter().any(|known| known == *id) {
                            now.app.start_with_hub.push((*id).to_owned());
                        }
                    }
                    file = now.clone();
                    Ok(())
                })?;
            }
            for (id, identity) in taken {
                match login.remove(identity) {
                    Ok(()) => out.push_str(&format!(
                        "{id}: its own login entry ({}) is now started by hub-desktop\n",
                        identity.app
                    )),
                    Err(e) => out.push_str(&format!(
                        "{id}: hub-desktop starts it too, but its own login entry stays: {e}\n"
                    )),
                }
            }
            out.push_str(&with_it(&file.app.start_with_hub));
        }
        Autostart::Off => {
            // What each app shows as its start with the login stays: the apps of the list get their own
            // value back.
            for id in &file.app.start_with_hub {
                let Some(identity) = catalog::find(id).and_then(catalog::Entry::kit_identity)
                else {
                    continue;
                };
                if login.of(identity) == Some(true) {
                    continue;
                }
                match login
                    .program(identity)
                    .and_then(|p| login.give(identity, &p))
                {
                    Ok(()) => out.push_str(&format!(
                        "{id}: starts with your login on its own again ({})\n",
                        identity.app
                    )),
                    Err(e) => out.push_str(&format!(
                        "{id}: does not start with your login any more: {e}\n"
                    )),
                }
            }
        }
        Autostart::Status => out.push_str(&with_it(&file.app.start_with_hub)),
    }
    Ok(out)
}

fn with_it(list: &[String]) -> String {
    if list.is_empty() {
        "hub-desktop starts no other hub (start_with_hub in config.json)\n".to_owned()
    } else {
        format!("hub-desktop starts with it: {}\n", list.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeSet;

    use appkit_scratch::Scratch;

    use super::*;

    /// The login of a machine that is made for the test.
    #[derive(Default)]
    struct FakeLogin {
        own: RefCell<bool>,
        values: RefCell<BTreeSet<&'static str>>,
        stuck: BTreeSet<&'static str>,
        calls: RefCell<Vec<String>>,
    }

    impl FakeLogin {
        fn with(values: &[&'static str]) -> Self {
            Self {
                values: RefCell::new(values.iter().copied().collect()),
                ..Self::default()
            }
        }
    }

    impl LoginEntries for FakeLogin {
        fn own(&self, what: Autostart) -> Result<String, String> {
            self.calls.borrow_mut().push(format!("own {what:?}"));
            match what {
                Autostart::On => *self.own.borrow_mut() = true,
                Autostart::Off => *self.own.borrow_mut() = false,
                Autostart::Status => {}
            }
            Ok(format!("own {}\n", *self.own.borrow()))
        }
        fn of(&self, identity: &'static AppIdentity) -> Option<bool> {
            Some(self.values.borrow().contains(identity.app))
        }
        fn remove(&self, identity: &'static AppIdentity) -> Result<(), String> {
            self.calls
                .borrow_mut()
                .push(format!("remove {}", identity.app));
            if self.stuck.contains(identity.app) {
                return Err("access denied".to_owned());
            }
            self.values.borrow_mut().remove(identity.app);
            Ok(())
        }
        fn give(&self, identity: &'static AppIdentity, program: &Path) -> Result<(), String> {
            self.calls
                .borrow_mut()
                .push(format!("give {} {}", identity.app, program.display()));
            self.values.borrow_mut().insert(identity.app);
            Ok(())
        }
        fn program(&self, identity: &'static AppIdentity) -> Result<PathBuf, String> {
            if self.stuck.contains(identity.app) {
                return Err("not installed".to_owned());
            }
            Ok(PathBuf::from(format!("C:/apps/{}.exe", identity.binary)))
        }
    }

    fn list(dir: &Path) -> Vec<String> {
        config::load_file(dir).unwrap().app.start_with_hub
    }

    #[test]
    fn on_takes_over_the_login_entries_of_the_hubs_and_leaves_the_others() {
        let dir = Scratch::new("hub-login-on");
        let login = FakeLogin::with(&["tasks-desktop", "rules-desktop", "pult", "other"]);
        let out = autostart(dir.path(), Autostart::On, &login).unwrap();
        assert!(*login.own.borrow());
        assert_eq!(list(dir.path()), ["tasks", "rules"]);
        // pult keeps its own (its window), and nothing that is not an app of the list is touched.
        assert_eq!(
            *login.values.borrow(),
            ["other", "pult"].into_iter().collect::<BTreeSet<_>>()
        );
        assert!(out.contains("tasks: its own login entry"), "{out}");
        assert!(
            out.contains("hub-desktop starts with it: tasks, rules"),
            "{out}"
        );
        // Its own value first, then the list, then the values of the apps.
        assert_eq!(
            *login.calls.borrow(),
            ["own On", "remove tasks-desktop", "remove rules-desktop"]
        );
    }

    #[test]
    fn on_while_an_app_edits_the_list_loses_neither_change() {
        let dir = Scratch::new("hub-login-race");
        let path = dir.path().to_owned();
        let hub = path.clone();
        config::an_app_adds_to_the_list_while(&path, "testing", move || {
            let login = FakeLogin::with(&["tasks-desktop"]);
            autostart(&hub, Autostart::On, &login).expect("autostart on");
        });
        let mut ids = list(&path);
        ids.sort();
        assert_eq!(ids, ["tasks", "testing"]);
    }

    #[test]
    fn on_a_second_time_names_each_hub_once_and_keeps_what_the_person_wrote() {
        let dir = Scratch::new("hub-login-again");
        std::fs::write(
            dir.path().join("config.json"),
            r#"{"web_root":"C:/w","start_with_hub":["testing"]}"#,
        )
        .unwrap();
        let login = FakeLogin::with(&["testing-desktop", "tasks-desktop"]);
        autostart(dir.path(), Autostart::On, &login).unwrap();
        autostart(dir.path(), Autostart::On, &login).unwrap();
        let file = config::load_file(dir.path()).unwrap();
        assert_eq!(file.app.start_with_hub, ["testing", "tasks"]);
        assert_eq!(file.app.web_root.as_deref(), Some(Path::new("C:/w")));
    }

    #[test]
    fn a_value_that_cannot_be_removed_stays_and_the_hub_is_on_the_list_all_the_same() {
        let dir = Scratch::new("hub-login-stuck");
        let login = FakeLogin {
            stuck: ["rules-desktop"].into_iter().collect(),
            ..FakeLogin::with(&["rules-desktop"])
        };
        let out = autostart(dir.path(), Autostart::On, &login).unwrap();
        assert_eq!(list(dir.path()), ["rules"]);
        assert!(login.values.borrow().contains("rules-desktop"));
        assert!(
            out.contains("its own login entry stays: access denied"),
            "{out}"
        );
    }

    #[test]
    fn nothing_to_take_over_writes_no_settings() {
        let dir = Scratch::new("hub-login-none");
        let login = FakeLogin::default();
        let out = autostart(dir.path(), Autostart::On, &login).unwrap();
        assert!(!dir.path().join("config.json").exists());
        assert!(out.contains("starts no other hub"), "{out}");
    }

    #[test]
    fn off_gives_the_apps_of_the_list_their_own_value_back_and_keeps_the_list() {
        let dir = Scratch::new("hub-login-off");
        let login = FakeLogin::with(&["tasks-desktop", "rules-desktop"]);
        autostart(dir.path(), Autostart::On, &login).unwrap();
        // rules got its own value meanwhile (its own `autostart on`): it is not written twice.
        login.values.borrow_mut().insert("rules-desktop");
        login.calls.borrow_mut().clear();
        let out = autostart(dir.path(), Autostart::Off, &login).unwrap();
        assert!(!*login.own.borrow());
        assert_eq!(
            *login.values.borrow(),
            ["rules-desktop", "tasks-desktop"]
                .into_iter()
                .collect::<BTreeSet<_>>()
        );
        assert_eq!(
            *login.calls.borrow(),
            ["own Off", "give tasks-desktop C:/apps/tasks-hub.exe"]
        );
        assert_eq!(list(dir.path()), ["tasks", "rules"]);
        assert!(
            out.contains("tasks: starts with your login on its own again"),
            "{out}"
        );
        let status = autostart(dir.path(), Autostart::Status, &login).unwrap();
        assert!(status.starts_with("own false"), "{status}");
    }

    #[test]
    fn off_says_which_app_cannot_get_its_value_back() {
        let dir = Scratch::new("hub-login-off-missing");
        std::fs::write(
            dir.path().join("config.json"),
            r#"{"start_with_hub":["testing"]}"#,
        )
        .unwrap();
        let login = FakeLogin {
            stuck: ["testing-desktop"].into_iter().collect(),
            ..FakeLogin::default()
        };
        let out = autostart(dir.path(), Autostart::Off, &login).unwrap();
        assert!(
            out.contains("testing: does not start with your login any more: not installed"),
            "{out}"
        );
    }

    #[test]
    fn the_names_of_hub_desktop_are_those_the_kit_writes_to() {
        assert_eq!(crate::IDENTITY, appkit_hub::loginstate::HUB_DESKTOP);
        for id in TAKES_OVER {
            let identity = catalog::find(id)
                .and_then(catalog::Entry::kit_identity)
                .unwrap();
            // The kit names an app in start_with_hub by its route_ns.
            assert_eq!(identity.route_ns, id);
        }
    }

    #[test]
    fn the_apps_taken_over_are_hubs_of_the_kit() {
        for id in TAKES_OVER {
            assert!(
                catalog::find(id).is_some_and(catalog::Entry::is_kit_hub),
                "{id}"
            );
        }
    }
}
