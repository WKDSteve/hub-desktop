//! The catalog holds copies of what other apps call themselves. They are on the disks and in the
//! environments of people, so a copy that drifts from the app is a hub that looks in the wrong place:
//! these tests pin the values, and the ports to the list that appkit keeps for all apps.

use appkit_testkit::ports::APP_PORTS;
use hub_desktop::catalog::{self, Native, AI_PORT, RULES, TASKS, TESTING};

#[test]
fn the_names_of_the_appkit_apps_are_the_ones_they_give_themselves() {
    assert_eq!(TASKS.app, "tasks-desktop");
    assert_eq!(TASKS.binary, "tasks-hub");
    assert_eq!(TASKS.var("INSTALL_DIR"), "TASKS_DESKTOP_INSTALL_DIR");
    assert_eq!(TASKS.var("CONFIG_DIR"), "TASKS_DESKTOP_CONFIG_DIR");
    assert_eq!(TASKS.route("hello"), "/api/tasks/hello");
    assert_eq!(RULES.app, "rules-desktop");
    assert_eq!(RULES.binary, "rules-hub");
    assert_eq!(RULES.var("INSTALL_DIR"), "RULES_DESKTOP_INSTALL_DIR");
    assert_eq!(RULES.route("hello"), "/api/rules/hello");
    assert_eq!(TESTING.app, "testing-desktop");
    assert_eq!(TESTING.binary, "testing-hub");
    assert_eq!(TESTING.var("INSTALL_DIR"), "TESTING_DESKTOP_INSTALL_DIR");
    assert_eq!(TESTING.route("hello"), "/api/testing/hello");
}

#[test]
fn every_port_of_the_catalog_is_in_the_list_of_appkit_under_the_same_app() {
    let owned = |port: u16, name: &str| {
        APP_PORTS
            .iter()
            .any(|p| p.port == port && p.owner.starts_with(name))
    };
    assert!(owned(TASKS.default_port, "tasks-desktop"));
    assert!(owned(RULES.default_port, "rules-desktop"));
    assert!(owned(TESTING.default_port, "testing-desktop"));
    assert!(owned(AI_PORT, "ai-desktop"));
    assert!(owned(hub_desktop::IDENTITY.default_port, "hub-desktop"));
    // Nothing in the catalog may use the ports of the hub itself.
    for entry in catalog::CATALOG {
        let port = match entry.native {
            Some(Native::AppKitHub { identity, .. }) => identity.default_port,
            Some(Native::LoopbackServer { default_port, .. }) => default_port,
            _ => continue,
        };
        assert!(![17610, 17611].contains(&port), "{}", entry.id);
    }
}

/// The variables that name the vault are the ones of the app: the hub reads them to say "needs setup".
#[test]
fn the_vault_variables_are_the_ones_of_the_app() {
    let vault_vars = |id: &str| match catalog::find(id).unwrap().native {
        Some(Native::AppKitHub { vault_vars, .. }) => vault_vars,
        other => panic!("{id}: {other:?}"),
    };
    assert_eq!(vault_vars("tasks"), ["TASKS_VAULT"]);
    assert_eq!(vault_vars("rules"), ["RULES_VAULT"]);
    assert_eq!(vault_vars("testing"), ["TESTING_REPOS_ROOT", "REPOS_DIR"]);
}
