//! What is on this machine: which apps are installed, which run, which need to be set up (concept 5).
//!
//! "Installed" is a look at files (`paths`), nothing else: no process list, no helper program. Whether
//! an app starts with the login is the one look at the registry, made by the kit (`autostart_of`), read only. "Running" is a look at a loopback port (`probe`), at all apps at once. The answer is cached
//! for a few seconds, so that a page that asks again and again costs one round of looks.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use appkit_core::identity::AppIdentity;
use appkit_core::registry;
use serde::Serialize;
use serde_json::Value;
use tokio::task::JoinSet;
use tokio::time::Instant;

use crate::catalog::{self, Entry, Hint, Mode, Native, Status, CATALOG};
use crate::config::Env;
use crate::overlay::Overlay;
use crate::paths::{self, Candidate};
use crate::probe::{self, Probe};

/// How long an answer is held before the machine is looked at again.
pub const CACHE_TTL: Duration = Duration::from_secs(10);

/// The larger a `config.json` of another app is, the less it is: it is read for one fact, and a file of
/// this size is no settings file.
const MAX_FOREIGN_CONFIG: u64 = 64 * 1024;

/// How the login entry of an app is read: `None` is "cannot be told". The program passes the kit's reader;
/// the default tells nothing, so that a test never looks at the registry of the machine.
pub type AutostartReader = fn(&'static AppIdentity) -> Option<bool>;

fn no_autostart(_: &'static AppIdentity) -> Option<bool> {
    None
}

/// Whether an app is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Run {
    Running,
    Stopped,
    /// The port of the app answers, but not as the app: it must not be started over it.
    PortBusy,
}

/// What the hub found out about one app. The paths stay in the program: they are what a start would
/// run, and they never go to the page.
#[derive(Debug, Clone)]
pub struct AppState {
    pub entry: &'static Entry,
    /// The program, if the native app is installed.
    pub native: Option<PathBuf>,
    /// The page of the web variant, if it is there.
    pub web: Option<PathBuf>,
    /// `None` for an app that has no way to be looked at (a program with a window of its own, a web
    /// variant only).
    pub run: Option<Run>,
    /// Why the app cannot be used yet (`no_vault`), as the app says it or as its settings show it.
    pub setup: Option<String>,
    /// The loopback port of a program that serves a page (ai-desktop): where it is, and where its
    /// page is once it runs.
    pub port: Option<u16>,
    /// The mode a person chose for the tile; none: the default of the catalog.
    pub chosen: Option<Mode>,
    /// Whether the installed native app starts with the login; `None`: not an app of the kit, not
    /// installed, or it cannot be told.
    pub autostart: Option<bool>,
    /// The build the running app says it is (`hello.build`, a short commit id); `None`: not running,
    /// an app that does not say it, or an answer of a shape we do not take.
    pub build: Option<String>,
}

impl AppState {
    #[must_use]
    pub fn installed(&self) -> bool {
        self.native.is_some() || self.web.is_some()
    }

    /// The mode a click opens: the default of the app, or the other one when only that is there.
    #[must_use]
    pub fn mode(&self) -> Mode {
        match (
            self.chosen.unwrap_or(self.entry.default_mode),
            self.native.is_some(),
            self.web.is_some(),
        ) {
            (Mode::Native, false, true) => Mode::Web,
            (Mode::Web, true, false) => Mode::Native,
            (mode, _, _) => mode,
        }
    }
}

/// An app as the page gets it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AppView {
    pub id: &'static str,
    pub name: &'static str,
    pub summary: &'static str,
    pub status: &'static str,
    pub installed: bool,
    pub native: bool,
    pub web: bool,
    pub default_mode: &'static str,
    pub run: Option<Run>,
    pub setup: Option<String>,
    /// The app runs and is one the hub can ask to stop (an app built on `appkit-hub`).
    pub stoppable: bool,
    /// The app starts with the login (an app of the kit only; `null` when it cannot be told).
    pub autostart: Option<bool>,
    /// The build of the running app, for the update badge of the page (`null` when it cannot be told).
    pub build: Option<String>,
    pub install: InstallView,
    pub repo: RepoView,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct InstallView {
    pub hint: &'static str,
    pub page: Option<&'static str>,
    pub file: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RepoView {
    pub owner: &'static str,
    pub name: &'static str,
    pub private: bool,
}

fn mode_word(mode: Mode) -> &'static str {
    match mode {
        Mode::Native => "native",
        Mode::Web => "web",
    }
}

/// The word the page gets for how far an app is.
#[must_use]
pub fn status_word(status: Status) -> &'static str {
    match status {
        Status::Available => "available",
        Status::Planned => "planned",
        Status::BuildOnly => "build_only",
    }
}

/// The way to get an app, as the page gets it.
#[must_use]
pub fn install_view(entry: &Entry) -> InstallView {
    InstallView {
        hint: match entry.install.hint {
            Hint::Release => "release",
            Hint::Clone => "clone",
            Hint::Planned => "planned",
            Hint::BuildYourself => "build_yourself",
        },
        page: entry.install.page,
        file: entry.install.file,
    }
}

/// Where the code of an app is, as the page gets it.
#[must_use]
pub fn repo_view(entry: &Entry) -> RepoView {
    RepoView {
        owner: entry.repo.owner,
        name: entry.repo.name,
        private: entry.repo.visibility == catalog::Visibility::Private,
    }
}

impl AppState {
    #[must_use]
    pub fn view(&self) -> AppView {
        let e = self.entry;
        AppView {
            id: e.id,
            name: e.name,
            summary: e.summary,
            status: status_word(e.status),
            installed: self.installed(),
            native: self.native.is_some(),
            web: self.web.is_some(),
            default_mode: mode_word(self.mode()),
            run: self.run,
            setup: self.setup.clone(),
            stoppable: self.run == Some(Run::Running)
                && self.native.is_some()
                && matches!(e.native, Some(Native::AppKitHub { .. })),
            autostart: self.autostart,
            build: self.build.clone(),
            install: install_view(e),
            repo: repo_view(e),
        }
    }
}

/// Looks at the machine, and remembers the answer for [`CACHE_TTL`].
pub struct Detector {
    env: Arc<Env>,
    /// The folder with the checkouts of the web variants, from the settings; none: the usual places.
    web_root: Option<PathBuf>,
    ttl: Duration,
    /// The modes people chose, by app id (`config.json`, `tiles`).
    modes: RwLock<BTreeMap<String, Mode>>,
    /// What the person hid and ordered (`catalog-overlay.json`).
    overlay: Overlay,
    autostart: AutostartReader,
    cache: Mutex<Option<(Instant, Arc<Vec<AppState>>)>>,
}

impl Detector {
    #[must_use]
    pub fn new(env: Env, web_root: Option<PathBuf>) -> Self {
        Self::with_ttl(env, web_root, CACHE_TTL)
    }

    #[must_use]
    pub fn with_ttl(env: Env, web_root: Option<PathBuf>, ttl: Duration) -> Self {
        Self {
            env: Arc::new(env),
            web_root,
            ttl,
            modes: RwLock::new(BTreeMap::new()),
            overlay: Overlay::default(),
            autostart: no_autostart,
            cache: Mutex::new(None),
        }
    }

    /// The chosen modes, from the settings.
    #[must_use]
    pub fn with_modes(self, modes: BTreeMap<String, Mode>) -> Self {
        if let Ok(mut held) = self.modes.write() {
            *held = modes;
        }
        self
    }

    /// The overlay of the person: which apps are not shown, and in which order the others are.
    #[must_use]
    pub fn with_overlay(mut self, overlay: Overlay) -> Self {
        self.overlay = overlay;
        self
    }

    /// How the login entry of an app is read (the kit's `autostart_of` in the program).
    #[must_use]
    pub fn with_autostart(mut self, reader: AutostartReader) -> Self {
        self.autostart = reader;
        self
    }

    /// Whether the person hid the app: it is not listed, not started and its page is not served.
    #[must_use]
    pub fn hides(&self, id: &str) -> bool {
        self.overlay.hides(id)
    }

    /// Choose (or, with `None`, forget) the mode a click opens for an app; the next look shows it.
    pub fn set_mode(&self, id: &str, mode: Option<Mode>) {
        if let Ok(mut held) = self.modes.write() {
            match mode {
                Some(mode) => held.insert(id.to_owned(), mode),
                None => held.remove(id),
            };
        }
        self.invalidate();
    }

    fn chosen(&self, id: &str) -> Option<Mode> {
        self.modes.read().ok()?.get(id).copied()
    }

    /// Forget what was found (an app was started: the next look must see it).
    pub fn invalidate(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            *cache = None;
        }
    }

    /// Every app of the catalog, in its order: from the cache when it is young, else from a look at the
    /// machine.
    pub async fn apps(&self) -> Arc<Vec<AppState>> {
        if let Ok(cache) = self.cache.lock()
            && let Some((at, held)) = cache.as_ref()
            && at.elapsed() < self.ttl
        {
            return Arc::clone(held);
        }
        let fresh = Arc::new(self.look().await);
        if let Ok(mut cache) = self.cache.lock() {
            *cache = Some((Instant::now(), Arc::clone(&fresh)));
        }
        fresh
    }

    /// One app, looked at now: not from the cache, and the cache is forgotten afterwards (what is found
    /// here is newer than what it holds). A start asks this: it must not act on a state that is ten
    /// seconds old. The login entry is not
    /// read (a start does not need it, and it costs a program run).
    pub async fn look_one(&self, entry: &'static Entry) -> AppState {
        let roots = web_roots(self.web_root.as_deref(), &self.env);
        let mut state = detect(entry, &self.env, &roots, no_autostart).await;
        state.chosen = self.chosen(entry.id);
        self.invalidate();
        state
    }

    /// One app, from the same look.
    pub async fn app(&self, id: &str) -> Option<AppState> {
        self.apps().await.iter().find(|a| a.entry.id == id).cloned()
    }

    /// All apps at once: the files are looked at first, then every port at the same time.
    async fn look(&self) -> Vec<AppState> {
        let roots = web_roots(self.web_root.as_deref(), &self.env);
        let mut tasks = JoinSet::new();
        for (index, entry) in CATALOG.iter().enumerate() {
            if self.overlay.hides(entry.id) {
                continue;
            }
            let env = Arc::clone(&self.env);
            let web = roots.clone();
            let chosen = self.chosen(entry.id);
            let autostart = self.autostart;
            tasks.spawn(async move {
                let mut state = detect(entry, &env, &web, autostart).await;
                state.chosen = chosen;
                (index, state)
            });
        }
        let mut found: Vec<Option<AppState>> = vec![None; CATALOG.len()];
        while let Some(done) = tasks.join_next().await {
            // A task that panicked leaves its app out of the list rather than the whole answer.
            if let Ok((index, state)) = done {
                found[index] = Some(state);
            }
        }
        let mut apps: Vec<AppState> = found.into_iter().flatten().collect();
        // Stable: the apps the person did not name stay in the order of the catalog, behind the named ones.
        apps.sort_by_key(|a| self.overlay.rank(a.entry.id));
        apps
    }
}

/// The folders that may hold the checkouts of the web variants: the one the settings name, else
/// `$REPOS_DIR` and `<home>\hub-web-apps`. A page is looked for below each, and checked against it.
fn web_roots(web_root: Option<&Path>, env: &Env) -> Vec<PathBuf> {
    if let Some(root) = web_root {
        return paths::usable_root(root.to_path_buf()).into_iter().collect();
    }
    let repos = env.path("REPOS_DIR").and_then(paths::usable_root);
    let apps = paths::home(env).map(|home| home.join("hub-web-apps"));
    repos.into_iter().chain(apps).collect()
}

/// The page of a web variant below the first root that has it.
fn find_web(entry: &Entry, roots: &[PathBuf]) -> Option<PathBuf> {
    let web = entry.web?;
    roots.iter().find_map(|root| {
        paths::existing_file(&Candidate {
            root: root.clone(),
            file: root.join(web.repo_dir).join(web.entry),
        })
    })
}

/// The program of an app of the catalog: the first candidate that is a file below its root.
fn find_native(native: &Native, env: &Env) -> Option<PathBuf> {
    match native {
        Native::AppKitHub { identity, .. } => {
            paths::existing_file(&paths::appkit_program(identity, env)?)
        }
        Native::GuiExe { candidates } | Native::LoopbackServer { candidates, .. } => candidates
            .iter()
            .filter_map(|template| paths::expand(template, env))
            .find_map(|candidate| paths::existing_file(&candidate)),
    }
}

async fn detect(
    entry: &'static Entry,
    env: &Env,
    web_roots: &[PathBuf],
    autostart: AutostartReader,
) -> AppState {
    let native = entry.native.as_ref().and_then(|n| find_native(n, env));
    let web = find_web(entry, web_roots);
    let mut port = None;
    let mut login = None;
    let mut build = None;
    let (run, setup) = match (&entry.native, &native) {
        (
            Some(Native::AppKitHub {
                identity,
                vault_vars,
            }),
            Some(_),
        ) => {
            // `reg.exe` is a program run: off the threads that answer requests.
            login = tokio::task::spawn_blocking(move || autostart(identity))
                .await
                .ok()
                .flatten();
            let (run, setup, seen) = appkit_state(identity, vault_vars, env).await;
            build = seen;
            (run, setup)
        }
        (
            Some(Native::LoopbackServer {
                port_var,
                default_port,
                ..
            }),
            Some(_),
        ) => {
            let at = ai_port(port_var, *default_port, env);
            port = Some(at);
            (Some(health_state(at).await), None)
        }
        _ => (None, None),
    };
    AppState {
        entry,
        native,
        web,
        run,
        setup,
        port,
        chosen: None,
        autostart: login,
        build,
    }
}

/// The port of a program that serves a page: the variable when it holds a usable port, else the default.
fn ai_port(var: &str, default: u16, env: &Env) -> u16 {
    env.get(var)
        .and_then(|v| v.to_string_lossy().parse::<u16>().ok())
        .filter(|p| *p >= 1024)
        .unwrap_or(default)
}

/// `/health` of ai-desktop: a hub of it answers with these three keys; anything else on the port is
/// somebody else's.
async fn health_state(port: u16) -> Run {
    match probe::get(port, "/health").await {
        Probe::Nothing => Run::Stopped,
        Probe::Answer { status: 200, body } if is_ai_health(&body) => Run::Running,
        Probe::Answer { .. } | Probe::Other => Run::PortBusy,
    }
}

fn is_ai_health(body: &str) -> bool {
    serde_json::from_str::<Value>(body).is_ok_and(|doc| {
        doc.get("status").is_some()
            && doc.get("model_router_ready").is_some()
            && doc.get("backends").is_some()
    })
}

/// A short code of the kind the apps give (`no_vault`): letters, digits and `_`, so that it can be shown
/// and never is a sentence or a path.
fn code_of(value: &Value) -> Option<String> {
    let code = value.as_str()?;
    (!code.is_empty()
        && code.len() <= 40
        && code
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'))
    .then(|| code.to_owned())
}

/// The build an app reports in `hello`: one to forty ASCII letters or digits (a short commit id), else
/// nothing. The page compares it with a release title, so it is held to the shape it is compared in.
fn build_of(value: &Value) -> Option<String> {
    let build = value.as_str()?;
    (!build.is_empty() && build.len() <= 40 && build.bytes().all(|b| b.is_ascii_alphanumeric()))
        .then(|| build.to_owned())
}

/// Whether an app built on `appkit-hub` runs, and whether it can be used: from its `hub.json` and its
/// `hello` when it runs, from its settings when it does not.
async fn appkit_state(
    identity: &'static appkit_core::identity::AppIdentity,
    vault_vars: &[&str],
    env: &Env,
) -> (Option<Run>, Option<String>, Option<String>) {
    let dir = appkit_hub::instance::config_folder(env, identity).ok();
    if let Some(dir) = &dir
        && let Some(hello) = running_hello(dir, identity).await
    {
        let setup = if hello["setup_required"].as_bool() == Some(true) {
            code_of(&hello["setup_reason"]).or_else(|| Some("setup_required".to_owned()))
        } else {
            None
        };
        return (Some(Run::Running), setup, build_of(&hello["build"]));
    }
    (
        Some(Run::Stopped),
        dir.and_then(|d| vault_problem(&d, vault_vars, env)),
        None,
    )
}

/// `hello` of the hub that `hub.json` names, if that hub is the one it says it is. The file is believed
/// only after the process on its port has said the same instance.
async fn running_hello(dir: &Path, identity: &appkit_core::identity::AppIdentity) -> Option<Value> {
    let record = registry::read_record(dir).ok().flatten()?;
    if record.app != identity.app || !probe::allowed_port(record.port, Some(identity.default_port))
    {
        return None;
    }
    let Probe::Answer { status: 200, body } =
        probe::get(record.port, &identity.route("hello")).await
    else {
        return None;
    };
    let doc: Value = serde_json::from_str(&body).ok()?;
    (doc["app"] == identity.app && doc["instance"].as_str() == Some(record.instance.as_str()))
        .then_some(doc)
}

/// Why a hub that does not run cannot be used once it does: its settings name no vault, or one that is
/// not a folder. Only the settings file and the variables are read; the folder is looked at, not entered.
fn vault_problem(dir: &Path, vault_vars: &[&str], env: &Env) -> Option<String> {
    let settings = read_foreign_config(&dir.join("config.json"));
    let from_file = settings
        .as_ref()
        .and_then(|doc| doc["vault"].as_str())
        .filter(|v| !v.is_empty())
        .map(PathBuf::from);
    let several = settings
        .as_ref()
        .and_then(|doc| doc["vaults"].as_array())
        .is_some_and(|list| !list.is_empty());
    match from_file.or_else(|| vault_vars.iter().find_map(|var| env.path(var))) {
        Some(vault) if vault.is_dir() => None,
        Some(_) => Some("vault_not_a_directory".to_owned()),
        None if several => None,
        None => Some("no_vault".to_owned()),
    }
}

/// A `config.json` of another app, read for one fact. A missing, large or broken file is no settings.
fn read_foreign_config(path: &Path) -> Option<Value> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FOREIGN_CONFIG {
        return None;
    }
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_health_answer_must_hold_the_three_keys() {
        assert!(is_ai_health(
            r#"{"status":"ok","model_router_ready":true,"backends":[]}"#
        ));
        for no in [
            "",
            "{}",
            r#"{"status":"ok"}"#,
            r#"{"status":"ok","model_router_ready":true}"#,
            "<html>",
            "[]",
        ] {
            assert!(!is_ai_health(no), "{no:?}");
        }
    }

    #[test]
    fn a_build_is_a_short_commit_id_or_nothing() {
        assert_eq!(
            build_of(&serde_json::json!("9f3b7ba")).as_deref(),
            Some("9f3b7ba")
        );
        for no in [
            serde_json::json!(null),
            serde_json::json!(""),
            serde_json::json!("9f3b 7ba"),
            serde_json::json!("v1.2"),
            serde_json::json!("<b>x</b>"),
            serde_json::json!("a".repeat(41)),
            serde_json::json!(7),
        ] {
            assert_eq!(build_of(&no), None, "{no}");
        }
    }

    #[test]
    fn a_reason_is_a_short_code_or_nothing() {
        for (value, want) in [
            (serde_json::json!("no_vault"), Some("no_vault")),
            (
                serde_json::json!("vault_not_a_directory"),
                Some("vault_not_a_directory"),
            ),
            (serde_json::json!(null), None),
            (serde_json::json!(""), None),
            (serde_json::json!("C:\\Users\\x\\vault"), None),
            (serde_json::json!("No vault is set"), None),
            (serde_json::json!("a".repeat(41)), None),
            (serde_json::json!(7), None),
        ] {
            assert_eq!(code_of(&value).as_deref(), want, "{value}");
        }
    }

    #[test]
    fn the_port_of_a_program_is_the_variable_when_usable_else_the_default() {
        let env = Env::from_pairs([("P", "9000"), ("LOW", "80"), ("BAD", "x"), ("BIG", "99999")]);
        assert_eq!(ai_port("P", 8080, &env), 9000);
        for var in ["LOW", "BAD", "BIG", "UNSET"] {
            assert_eq!(ai_port(var, 8080, &env), 8080, "{var}");
        }
    }
}
