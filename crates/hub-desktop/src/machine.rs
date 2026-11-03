//! Which machine the hub runs on: the private one, where it may install a native build, or the
//! workstation, where nothing unsigned is built, installed or started (EDR).
//!
//! The same variable as the nvim config (`lua/machine.lua`): `NVIM_MACHINE_ROLE` is `workstation` on the
//! workstation and unset (or `default`) elsewhere. A value of another kind, or an unset one on a machine that
//! looks like a managed one (`USERDOMAIN` `AzureAD`, a computer name `TRI-...`), is unclear, and unclear
//! counts as the workstation: the hub never installs on a guess.

use crate::config::Env;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Private,
    Workstation,
    Unclear,
}

impl Role {
    /// Read once when the hub starts; a variable set later needs a new start (as for every program).
    #[must_use]
    pub fn of(env: &Env) -> Self {
        let read = |name: &str| {
            env.get(name)
                .map(|v| v.to_string_lossy().trim().to_ascii_lowercase())
                .unwrap_or_default()
        };
        match read("NVIM_MACHINE_ROLE").as_str() {
            "workstation" => Self::Workstation,
            "" | "default" => {
                let managed =
                    read("USERDOMAIN") == "azuread" || read("COMPUTERNAME").starts_with("tri-");
                if managed {
                    Self::Unclear
                } else {
                    Self::Private
                }
            }
            _ => Self::Unclear,
        }
    }

    /// Whether a native build may be installed here.
    #[must_use]
    pub fn installs(self) -> bool {
        self == Self::Private
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn role(pairs: &[(&str, &str)]) -> Role {
        Role::of(&Env::from_pairs(pairs.iter().copied()))
    }

    #[test]
    fn only_a_machine_that_says_nothing_or_default_and_looks_private_is_private() {
        assert_eq!(role(&[]), Role::Private);
        assert_eq!(role(&[("NVIM_MACHINE_ROLE", "default")]), Role::Private);
        assert_eq!(role(&[("NVIM_MACHINE_ROLE", " Default ")]), Role::Private);
        assert_eq!(role(&[("COMPUTERNAME", "STEVESPC")]), Role::Private);
        assert!(Role::Private.installs());
    }

    #[test]
    fn the_workstation_says_so_in_any_case() {
        for word in ["workstation", "WORKSTATION", " Workstation "] {
            assert_eq!(
                role(&[("NVIM_MACHINE_ROLE", word)]),
                Role::Workstation,
                "{word}"
            );
        }
        assert!(!Role::Workstation.installs());
    }

    #[test]
    fn a_typo_or_a_managed_machine_without_the_variable_is_unclear_and_installs_nothing() {
        for pairs in [
            vec![("NVIM_MACHINE_ROLE", "workstaton")],
            vec![("NVIM_MACHINE_ROLE", ":NVIM_MACHINE_ROLE")],
            vec![("USERDOMAIN", "AzureAD")],
            vec![("COMPUTERNAME", "TRI-12345")],
            vec![("NVIM_MACHINE_ROLE", "default"), ("COMPUTERNAME", "tri-x")],
        ] {
            assert_eq!(role(&pairs), Role::Unclear, "{pairs:?}");
        }
        assert!(!Role::Unclear.installs());
        // A workstation that says so is a workstation, whatever its name.
        assert_eq!(
            role(&[
                ("NVIM_MACHINE_ROLE", "workstation"),
                ("USERDOMAIN", "AzureAD")
            ]),
            Role::Workstation
        );
    }
}
