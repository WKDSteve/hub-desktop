//! Where an app may be looked for, and what counts as found (concept 4.2, 5.1).
//!
//! A path of the catalog is a template: one of the placeholders below at its start and plain folder
//! names after it. The result must be an absolute path to a file that, once every link on the way is
//! resolved, still lies below its root. Whatever a junction, a symbolic link, a `..` or a network share
//! points at beyond the root is not found. Nothing here starts a process or reads the registry.

use std::path::{Component, Path, PathBuf};

use crate::config::Env;

/// The roots a template may start with.
const PLACEHOLDERS: [&str; 4] = ["LOCALAPPDATA", "APPDATA", "PROGRAMFILES", "HOME"];

/// A file that may exist, and the folder it has to stay in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub root: PathBuf,
    pub file: PathBuf,
}

/// A network share (`\\server\share`), a device path (`\\?\`, `\\.\`) or the same with slashes.
fn is_network_or_device(path: &Path) -> bool {
    let text = path.as_os_str().to_string_lossy();
    text.starts_with(r"\\") || text.starts_with("//")
}

/// A folder of the environment that can be a root: absolute, not a share, not a device, no `..`.
pub(crate) fn usable_root(path: PathBuf) -> Option<PathBuf> {
    let plain = path.is_absolute()
        && !is_network_or_device(&path)
        && path
            .components()
            .all(|c| !matches!(c, Component::ParentDir | Component::CurDir));
    plain.then_some(path)
}

/// The value of a placeholder: `HOME` is the home folder of the person (`USERPROFILE` on Windows).
fn root_of(name: &str, env: &Env) -> Option<PathBuf> {
    let value = if name == "HOME" {
        env.path("USERPROFILE").or_else(|| env.path("HOME"))
    } else {
        env.path(name)
    };
    value.and_then(usable_root)
}

/// The home folder of the person.
pub(crate) fn home(env: &Env) -> Option<PathBuf> {
    root_of("HOME", env)
}

/// One part of a path below a root: a plain name, nothing that climbs, names a drive or a stream, or
/// is a placeholder again.
fn plain_part(part: &str) -> bool {
    !part.is_empty()
        && part != "."
        && part != ".."
        && !part.ends_with([' ', '.'])
        && !part
            .chars()
            .any(|c| c.is_control() || r#"/\:*?"<>|{}"#.contains(c))
}

/// A template of the catalog as a candidate. `None` when the template is not of the allowed shape or
/// the placeholder has no usable value on this machine.
#[must_use]
pub fn expand(template: &str, env: &Env) -> Option<Candidate> {
    let rest = template.strip_prefix('{')?;
    let (name, below) = rest.split_once('}')?;
    if !PLACEHOLDERS.contains(&name) {
        return None;
    }
    let below = below.strip_prefix(['\\', '/'])?;
    let root = root_of(name, env)?;
    let mut file = root.clone();
    for part in below.split(['\\', '/']) {
        if !plain_part(part) {
            return None;
        }
        file.push(part);
    }
    Some(Candidate { root, file })
}

/// The install folder of an app built on `appkit-hub` and the program in it: `<PREFIX>_INSTALL_DIR`
/// names the folder, else it is `{LOCALAPPDATA}\<app>\bin` (the logic of `appkit-hub`'s `install`).
#[must_use]
pub fn appkit_program(
    identity: &appkit_core::identity::AppIdentity,
    env: &Env,
) -> Option<Candidate> {
    let program = identity.program_file();
    if let Some(dir) = env.path(&identity.var("INSTALL_DIR")) {
        // The folder the person named is the root: the program has to be in it, not behind a link.
        let root = usable_root(dir)?;
        let file = root.join(program);
        return Some(Candidate { root, file });
    }
    let root = root_of("LOCALAPPDATA", env)?;
    let file = root.join(identity.app).join("bin").join(program);
    Some(Candidate { root, file })
}

/// The file of a candidate, resolved, if it is there, is a file and lies below its root after every
/// link on the way is followed. This is the only place that says "found".
#[must_use]
pub fn existing_file(candidate: &Candidate) -> Option<PathBuf> {
    let root = std::fs::canonicalize(&candidate.root).ok()?;
    let file = std::fs::canonicalize(&candidate.file).ok()?;
    // `canonicalize` of a share or a device gives `\\?\UNC\…`: it is no root of ours either way.
    if is_network_or_device(&candidate.file) || !file.starts_with(&root) {
        return None;
    }
    std::fs::metadata(&file).ok()?.is_file().then_some(file)
}

#[cfg(test)]
mod tests {
    use appkit_scratch::Scratch;

    use super::*;

    fn env_of(dir: &Path) -> Env {
        Env::from_pairs([
            ("LOCALAPPDATA", dir.join("Local")),
            ("APPDATA", dir.join("Roaming")),
            ("PROGRAMFILES", dir.join("Program Files")),
            ("USERPROFILE", dir.join("Home")),
        ])
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"x").unwrap();
    }

    #[test]
    fn a_template_below_a_placeholder_is_a_candidate() {
        let scratch = Scratch::new("hub-paths-expand");
        let env = env_of(scratch.path());
        let got = expand(r"{LOCALAPPDATA}\docmap-desktop\docmap-desktop.exe", &env).unwrap();
        assert_eq!(got.root, scratch.path().join("Local"));
        assert_eq!(
            got.file,
            scratch
                .path()
                .join("Local/docmap-desktop/docmap-desktop.exe")
        );
        let pf = expand(r"{PROGRAMFILES}\x\y.exe", &env).unwrap();
        assert_eq!(pf.root, scratch.path().join("Program Files"));
        let home = expand("{HOME}/tool.exe", &env).unwrap();
        assert_eq!(home.root, scratch.path().join("Home"));
    }

    #[test]
    fn a_template_of_another_shape_is_refused() {
        let scratch = Scratch::new("hub-paths-shape");
        let env = env_of(scratch.path());
        for bad in [
            "",
            r"C:\Windows\System32\cmd.exe",
            r"\\server\share\x.exe",
            r"{TEMP}\x.exe",
            r"{LOCALAPPDATA}",
            r"{LOCALAPPDATA}x.exe",
            r"{LOCALAPPDATA}\..\x.exe",
            r"{LOCALAPPDATA}\a\..\..\x.exe",
            r"{LOCALAPPDATA}\.\x.exe",
            r"{LOCALAPPDATA}\\x.exe",
            r"{LOCALAPPDATA}\a:b\x.exe",
            r"{LOCALAPPDATA}\{APPDATA}\x.exe",
            r"{LOCALAPPDATA}\x.exe ",
            "{LOCALAPPDATA}\\a\nb.exe",
        ] {
            assert!(expand(bad, &env).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn a_root_that_is_missing_relative_a_share_or_climbing_is_no_root() {
        let none = Env::from_pairs([("APPDATA", "C:/a")]);
        assert!(expand(r"{LOCALAPPDATA}\x.exe", &none).is_none());
        for bad in [
            "relative/dir",
            r"\\server\share",
            "//server/share",
            r"\\?\C:\x",
            "C:/a/../b",
        ] {
            let env = Env::from_pairs([("LOCALAPPDATA", bad)]);
            assert!(expand(r"{LOCALAPPDATA}\x.exe", &env).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn a_file_that_is_there_is_found_and_one_that_is_not_or_is_a_folder_is_not() {
        let scratch = Scratch::new("hub-paths-found");
        let env = env_of(scratch.path());
        let exe = scratch
            .path()
            .join("Local/docmap-desktop/docmap-desktop.exe");
        let cand = expand(r"{LOCALAPPDATA}\docmap-desktop\docmap-desktop.exe", &env).unwrap();
        assert!(existing_file(&cand).is_none(), "not there yet");
        touch(&exe);
        assert_eq!(
            existing_file(&cand).unwrap(),
            std::fs::canonicalize(&exe).unwrap()
        );
        // A folder with the name of the program is not the program.
        let dir_cand = expand(r"{LOCALAPPDATA}\docmap-desktop", &env).unwrap();
        assert!(existing_file(&dir_cand).is_none());
    }

    /// A folder of the root that is a link to somewhere else: what is behind it is not below the root.
    #[test]
    fn a_link_that_leaves_the_root_finds_nothing() {
        let scratch = Scratch::new("hub-paths-link");
        let env = env_of(scratch.path());
        let outside = scratch.path().join("Outside/app");
        touch(&outside.join("evil.exe"));
        std::fs::create_dir_all(scratch.path().join("Local")).unwrap();
        let link = scratch.path().join("Local/app");
        make_dir_link(&outside, &link);
        let cand = expand(r"{LOCALAPPDATA}\app\evil.exe", &env).unwrap();
        assert!(
            cand.file.exists(),
            "the path itself resolves, through the link"
        );
        assert!(existing_file(&cand).is_none());
    }

    /// The root may itself be reached through a link (the profile folder of a redirected user): both
    /// sides are resolved, so the program is found.
    #[test]
    fn a_root_that_is_a_link_is_fine() {
        let scratch = Scratch::new("hub-paths-rootlink");
        let real = scratch.path().join("Real");
        touch(&real.join("app/tool.exe"));
        let via = scratch.path().join("Via");
        make_dir_link(&real, &via);
        let env = Env::from_pairs([("LOCALAPPDATA", via)]);
        let cand = expand(r"{LOCALAPPDATA}\app\tool.exe", &env).unwrap();
        assert!(existing_file(&cand).is_some());
    }

    #[cfg(windows)]
    fn make_dir_link(target: &Path, link: &Path) {
        // A junction needs no privilege, unlike a symbolic link.
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link.to_string_lossy().replace('/', "\\"))
            .arg(target.to_string_lossy().replace('/', "\\"))
            .stdout(std::process::Stdio::null())
            .status()
            .expect("cmd runs");
        assert!(status.success(), "mklink /J failed");
    }

    #[cfg(unix)]
    fn make_dir_link(target: &Path, link: &Path) {
        std::os::unix::fs::symlink(target, link).unwrap();
    }

    #[test]
    fn the_program_of_an_appkit_app_is_in_its_install_folder_or_where_the_override_says() {
        let scratch = Scratch::new("hub-paths-appkit");
        let id = &crate::catalog::TASKS;
        let program = id.program_file();
        let env = env_of(scratch.path());
        let default = appkit_program(id, &env).unwrap();
        assert_eq!(default.root, scratch.path().join("Local"));
        assert_eq!(
            default.file,
            scratch
                .path()
                .join("Local/tasks-desktop/bin")
                .join(&program)
        );

        let custom = scratch.path().join("Custom");
        let env = Env::from_pairs([
            ("LOCALAPPDATA", scratch.path().join("Local")),
            ("TASKS_DESKTOP_INSTALL_DIR", custom.clone()),
        ]);
        let over = appkit_program(id, &env).unwrap();
        assert_eq!(over.root, custom);
        assert_eq!(over.file, custom.join(&program));

        // An override that is relative or a share is no folder to look in.
        for bad in ["relative", r"\\server\share\bin"] {
            let env = Env::from_pairs([
                ("LOCALAPPDATA", scratch.path().join("Local")),
                ("TASKS_DESKTOP_INSTALL_DIR", PathBuf::from(bad)),
            ]);
            assert!(appkit_program(id, &env).is_none(), "{bad}");
        }
    }
}
