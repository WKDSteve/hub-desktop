//! `hub-desktop autostart on | off | status`: hub-desktop starts with the login, and with it the hubs
//! of the apps in `start_with_hub` (`config.json`).
//!
//! So that there is one icon in the notification area, not one program per app, `on` takes the login
//! entries of the apps over: an app of [`TAKES_OVER`] whose own value is in the `Run` key is put into
//! `start_with_hub` and its value is removed, in that order (a failure leaves a hub started twice, which
//! its `start` answers with "already running", never none). `off` removes the value of hub-desktop
//! only; it gives the apps no value back (each app can turn its own on again: `<binary> autostart on`).
//!
//! The registry is reached through [`LoginEntries`]: the program passes [`SystemLogin`] (the kit's
//! `reg.exe` calls), the tests a fake, so that no test ever touches the login of the machine.

use std::path::Path;

use appkit_core::identity::AppIdentity;
use appkit_hub::background::{Autostart, Background};
use appkit_hub::loginitems::LoginApp;

use crate::catalog;
use crate::config;

/// The apps whose own login entry `on` takes over: their value starts their hub (`<binary> start`). pult is
/// not one: its value starts its window, which has an icon of its own.
pub const TAKES_OVER: [&str; 3] = ["tasks", "rules", "testing"];

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
}

/// `autostart on | off | status` with the settings in `dir`. Returns what to print.
///
/// # Errors
///
/// A text for the person at the terminal: the value of hub-desktop could not be written or removed, or
/// the settings could not be read or written.
pub fn autostart(dir: &Path, what: Autostart, login: &dyn LoginEntries) -> Result<String, String> {
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
                if !file.app.start_with_hub.iter().any(|known| known == id) {
                    file.app.start_with_hub.push(id.to_owned());
                }
                taken.push((id, identity));
            }
            if !taken.is_empty() {
                // The list first: a value that cannot be removed then starts a hub twice, never none.
                std::fs::create_dir_all(dir)
                    .map_err(|e| format!("cannot create the config folder: {e}"))?;
                config::save_file(dir, &file)?;
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
            if !file.app.start_with_hub.is_empty() {
                out.push_str(&format!(
                    "the hubs of {} do not start with your login either; to start one on its own: \
                     `<program of the app> autostart on`\n",
                    file.app.start_with_hub.join(", ")
                ));
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
    fn off_removes_the_own_value_only_and_gives_the_apps_nothing_back() {
        let dir = Scratch::new("hub-login-off");
        let login = FakeLogin::with(&["tasks-desktop"]);
        autostart(dir.path(), Autostart::On, &login).unwrap();
        let out = autostart(dir.path(), Autostart::Off, &login).unwrap();
        assert!(!*login.own.borrow());
        assert!(login.values.borrow().is_empty());
        assert_eq!(list(dir.path()), ["tasks"]);
        assert!(out.contains("tasks do not start with your login"), "{out}");
        let status = autostart(dir.path(), Autostart::Status, &login).unwrap();
        assert!(status.starts_with("own false"), "{status}");
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
