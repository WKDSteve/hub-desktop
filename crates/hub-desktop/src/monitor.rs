//! The monitor: a light per repository, read from an export folder of testing.nvim (`testing-export/1`).
//!
//! testing.nvim owns the history and the start of a run; the hub keeps none of it and starts nothing here
//! (concept 10, M3). It reads one folder the person named (`testing_export` in `config.json`) and follows the
//! rules for a reader of `docs/OUTPUT-FORMATS.md` of testing.nvim:
//!
//! - the manifest `testing-export.json` first, and only the file it lists (`fleet.json`) with the size it says;
//! - a higher `schema` major, a document of another `kind`, a document bigger than 8 MiB, and a fleet that is
//!   not in the export form or whose data classes were not asked are refused;
//! - an unknown field is ignored, a missing or `null` value is unknown, never 0.
//!
//! Nothing a foreign file says reaches the page as it is: every word is mapped to a word of this module,
//! every text is cut. The reason a folder cannot be read is a fixed sentence, never a path.
//!
//! The red cases of one repository (`repos/<key>.json`, kind `repo`) are read the same way, on request: only a file
//! the manifest lists, of the size it says, whose `repo.key` is the key asked for.
//!
//! The check of `sha256` of the manifest is not made on purpose (the hub runs nothing from the export, and the
//! manifest is written last, so the size catches a half-written export); the size is checked.

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

/// The folder's documents the hub reads; the manifest names the rest.
const MANIFEST: &str = "testing-export.json";
const FLEET: &str = "fleet.json";

/// A document bigger than this is refused (the contract's limit).
const MAX_BYTES: u64 = 8 * 1024 * 1024;
/// More rows than this are not shown.
const MAX_ROWS: usize = 500;
/// A name longer than this is cut.
const MAX_NAME: usize = 80;

/// The highest major version of `testing-export` this hub reads.
const MAJOR: u64 = 1;

/// More red cases than this are not shown (the document holds at most 500).
const MAX_CASES: usize = 200;
/// A failure text (message, expected, actual) longer than this is cut (the contract cuts at 4 KiB).
const MAX_TEXT: usize = 4096;
/// A file, a case name or a describe title longer than this is cut.
const MAX_LABEL: usize = 300;
/// At most this many describe titles of a case.
const MAX_DESCRIBE: usize = 10;
/// A key longer than this is no key of the export.
const MAX_KEY: usize = 120;

/// How a repository stands, worst first when sources disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Light {
    Red,
    Amber,
    Green,
    /// No source says anything.
    Unknown,
}

/// One repository of the fleet, reduced to what the page shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepoLight {
    pub key: String,
    pub name: String,
    pub light: Light,
    /// The last local result: `green`, `green_partial`, `red`; `None`: no result (or not read here).
    pub local: Option<&'static str>,
    /// Red cases of that result.
    pub fail: Option<u64>,
    /// Spec files whose result is outdated.
    pub stale: Option<u64>,
    /// The last local pipeline run: `green`, `green_partial`, `red`, `error`, `infra`.
    pub ci_local: Option<&'static str>,
    /// The latest GitHub run for the head: a state word of the contract, or `None` when not asked.
    pub ci_remote: Option<&'static str>,
    /// When the local result was made, as the document says (UTC).
    pub ts: Option<String>,
}

/// What the page gets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Reading {
    /// No folder named: the page shows nothing of the monitor.
    NotConfigured,
    /// The folder is there but cannot be read; the reason is one of the fixed sentences.
    Problem { problem: &'static str },
    Ready {
        generated_at: Option<String>,
        /// `false`: the export was made where nothing runs (the workstation); the local columns are unknown.
        local_read: bool,
        repos: Vec<RepoLight>,
    },
}

/// One red case of the last local run, as the page shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RedCase {
    /// The spec file, relative to the repository (as the export writes it).
    pub file: String,
    pub line: Option<u64>,
    /// `fail`, `error`, `timeout`, `crash`, `xpass`.
    pub status: &'static str,
    pub name: String,
    /// The describe titles, outermost first.
    pub describe: Vec<String>,
    /// The texts of the failure, redacted by testing.nvim; line breaks kept, shown as text only.
    pub message: Option<String>,
    pub expected: Option<String>,
    pub actual: Option<String>,
    pub flaky: bool,
}

/// What the page gets for the red cases of one repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CasesReading {
    NotConfigured,
    Problem {
        problem: &'static str,
    },
    Ready {
        key: String,
        /// `false`: the export was made where nothing runs; there are no cases to tell.
        local_read: bool,
        /// All red cases of the last run (`None`: unknown).
        cases_red: Option<u64>,
        /// Red cases that are not in `cases` (left out by the export or by the hub).
        cases_omitted: Option<u64>,
        cases: Vec<RedCase>,
    },
}

/// Why the folder is not read (the words of the page; no path in them).
mod why {
    pub const MISSING: &str = "the export folder has no testing-export.json";
    pub const TOO_BIG: &str = "a document of the export is bigger than the hub reads";
    pub const UNREADABLE: &str = "the export cannot be read";
    pub const NOT_JSON: &str = "a document of the export is not JSON";
    pub const NOT_AN_EXPORT: &str = "testing-export.json is not a manifest of testing-export";
    pub const TOO_NEW: &str = "the export is of a newer schema than this hub reads";
    pub const NO_FLEET: &str = "the manifest does not list fleet.json";
    pub const SIZE: &str =
        "fleet.json is not the size the manifest says (an export in the middle of a write?)";
    pub const NOT_A_FLEET: &str = "fleet.json is not a fleet of the export form";
    pub const CLASSES: &str = "fleet.json was made without asking the data classes";
    pub const NO_REPO: &str = "the export has no document for this repository";
    pub const REPO_SIZE: &str =
        "the document of the repository is not the size the manifest says (an export in the middle of a write?)";
    pub const NOT_A_REPO: &str = "the document of the repository is not one of the export form";
}

/// The monitor of a hub: the folder, or none.
#[derive(Debug, Clone, Default)]
pub struct Monitor {
    dir: Option<PathBuf>,
}

impl Monitor {
    #[must_use]
    pub fn new(dir: Option<PathBuf>) -> Self {
        Self { dir }
    }

    /// Read the folder now (a few small files; the page asks when it shows the monitor).
    #[must_use]
    pub fn read(&self) -> Reading {
        let Some(dir) = &self.dir else {
            return Reading::NotConfigured;
        };
        match read_dir(dir) {
            Ok(ready) => ready,
            Err(problem) => Reading::Problem { problem },
        }
    }

    /// The red cases of the repository `key` of the fleet, read now.
    #[must_use]
    pub fn cases(&self, key: &str) -> CasesReading {
        let Some(dir) = &self.dir else {
            return CasesReading::NotConfigured;
        };
        match read_cases(dir, key) {
            Ok(ready) => ready,
            Err(problem) => CasesReading::Problem { problem },
        }
    }
}

/// A key of the export: `<name>-<hex>`, letters, digits, `.`, `_` and `-`, not starting with a dot. Anything else
/// is no file name of `repos/` and is never joined to a path.
fn plain_key(key: &str) -> bool {
    (1..=MAX_KEY).contains(&key.len())
        && !key.starts_with('.')
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn read_cases(dir: &Path, key: &str) -> Result<CasesReading, &'static str> {
    if !plain_key(key) {
        return Err(why::NO_REPO);
    }
    let (text, _) = read_text(&dir.join(MANIFEST))?;
    let manifest: Value = serde_json::from_str(&text).map_err(|_| why::NOT_JSON)?;
    check_head(&manifest, "manifest", why::NOT_AN_EXPORT)?;
    let path = format!("repos/{key}.json");
    let listed = manifest
        .get("files")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|f| f.get("path").and_then(Value::as_str) == Some(path.as_str()))
        .ok_or(why::NO_REPO)?;
    let (text, size) = match read_text(&dir.join("repos").join(format!("{key}.json"))) {
        Err(problem) if problem == why::MISSING => return Err(why::NO_REPO),
        other => other?,
    };
    if listed.get("bytes").and_then(Value::as_u64) != Some(size) {
        return Err(why::REPO_SIZE);
    }
    let doc: Value = serde_json::from_str(&text).map_err(|_| why::NOT_JSON)?;
    check_head(&doc, "repo", why::NOT_A_REPO)?;
    if doc.get("form").and_then(Value::as_str) != Some("export")
        || doc.pointer("/repo/key").and_then(Value::as_str) != Some(key)
    {
        return Err(why::NOT_A_REPO);
    }
    let local_read = doc.get("local_read").and_then(Value::as_bool) != Some(false);
    let all: Vec<RedCase> = doc
        .get("cases")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(red_case)
        .collect();
    let dropped = all.len().saturating_sub(MAX_CASES) as u64;
    let mut cases: Vec<RedCase> = all.into_iter().take(MAX_CASES).collect();
    // By file and line, as a person goes through them.
    cases.sort_by(|a, b| a.file.cmp(&b.file).then_with(|| a.line.cmp(&b.line)));
    // Without a local read the counts are unknown, whatever integers the document holds (the contract).
    let (cases_red, cases_omitted) = if local_read {
        (
            count(doc.get("cases_red")),
            count(doc.get("cases_omitted")).map(|n| n + dropped),
        )
    } else {
        (None, None)
    };
    Ok(CasesReading::Ready {
        key: key.to_owned(),
        local_read,
        cases_red,
        cases_omitted,
        cases: if local_read { cases } else { Vec::new() },
    })
}

/// A text of the document cut to `max` characters, without control characters; `keep_lines` keeps line
/// breaks and tabs (a failure text), else they go too.
fn cut(value: Option<&Value>, max: usize, keep_lines: bool) -> Option<String> {
    let raw = value?.as_str()?;
    let clean: String = raw
        .chars()
        .filter(|c| !c.is_control() || (keep_lines && matches!(c, '\n' | '\t')))
        .take(max)
        .collect();
    (!clean.is_empty()).then_some(clean)
}

fn red_word(word: &str) -> Option<&'static str> {
    match word {
        "fail" => Some("fail"),
        "error" => Some("error"),
        "timeout" => Some("timeout"),
        "crash" => Some("crash"),
        "xpass" => Some("xpass"),
        _ => None,
    }
}

fn red_case(value: &Value) -> Option<RedCase> {
    let status = red_word(value.get("status")?.as_str()?)?;
    Some(RedCase {
        file: cut(value.get("file"), MAX_LABEL, false)?,
        line: count(value.get("line")).filter(|n| *n > 0),
        status,
        name: cut(value.get("name"), MAX_LABEL, false).unwrap_or_default(),
        describe: value
            .get("describe")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|d| cut(Some(d), MAX_LABEL, false))
            .take(MAX_DESCRIBE)
            .collect(),
        message: cut(value.get("message"), MAX_TEXT, true),
        expected: cut(value.get("expected"), MAX_TEXT, true),
        actual: cut(value.get("actual"), MAX_TEXT, true),
        flaky: value.get("flaky").and_then(Value::as_bool) == Some(true),
    })
}

/// The text of a file no bigger than [`MAX_BYTES`]; `size` is what the file says.
fn read_text(path: &Path) -> Result<(String, u64), &'static str> {
    let meta = match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => meta,
        Ok(_) => return Err(why::UNREADABLE),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(why::MISSING),
        Err(_) => return Err(why::UNREADABLE),
    };
    if meta.len() > MAX_BYTES {
        return Err(why::TOO_BIG);
    }
    // The file may have grown since the look at it: read no more than the limit plus one byte.
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(MAX_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|_| why::UNREADABLE)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(why::TOO_BIG);
    }
    let size = bytes.len() as u64;
    let text = String::from_utf8(bytes).map_err(|_| why::NOT_JSON)?;
    Ok((text, size))
}

/// The major of `testing-export/<n>`.
fn major_of(doc: &Value) -> Option<u64> {
    let schema = doc.get("schema")?.as_str()?;
    let digits = schema.strip_prefix("testing-export/")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// A document of the contract: the schema is ours and not too new, the kind is the one asked for.
fn check_head(doc: &Value, kind: &str, not_it: &'static str) -> Result<(), &'static str> {
    let major = major_of(doc).ok_or(not_it)?;
    if major > MAJOR {
        return Err(why::TOO_NEW);
    }
    if doc.get("kind").and_then(Value::as_str) != Some(kind) {
        return Err(not_it);
    }
    Ok(())
}

fn read_dir(dir: &Path) -> Result<Reading, &'static str> {
    let (text, _) = read_text(&dir.join(MANIFEST))?;
    let manifest: Value = serde_json::from_str(&text).map_err(|_| why::NOT_JSON)?;
    check_head(&manifest, "manifest", why::NOT_AN_EXPORT)?;
    let listed = manifest
        .get("files")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|f| f.get("path").and_then(Value::as_str) == Some(FLEET))
        .ok_or(why::NO_FLEET)?;

    let (text, size) = read_text(&dir.join(FLEET))?;
    // The manifest is written last: a fleet of another size is one that is not the exported one.
    if listed.get("bytes").and_then(Value::as_u64) != Some(size) {
        return Err(why::SIZE);
    }
    let fleet: Value = serde_json::from_str(&text).map_err(|_| why::NOT_JSON)?;
    check_head(&fleet, "fleet", why::NOT_A_FLEET)?;
    if fleet.get("form").and_then(Value::as_str) != Some("export") {
        return Err(why::NOT_A_FLEET);
    }
    // An export that did not ask the data classes may hold what must not leave the machine.
    if fleet.pointer("/classes/checked").and_then(Value::as_bool) != Some(true) {
        return Err(why::CLASSES);
    }
    let mut seen = HashSet::new();
    let mut repos: Vec<RepoLight> = fleet
        .get("repos")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(row)
        // The page keys its rows by the key: a repeated one (a clone twice, a hand-made file) is shown once.
        .filter(|r| seen.insert(r.key.clone()))
        .take(MAX_ROWS)
        .collect();
    // Red first, then by name: what needs a look is on top.
    repos.sort_by(|a, b| a.light.cmp(&b.light).then_with(|| a.name.cmp(&b.name)));
    Ok(Reading::Ready {
        generated_at: stamp(fleet.get("generated_at")),
        local_read: fleet.get("local_read").and_then(Value::as_bool) != Some(false),
        repos,
    })
}

/// A UTC time as the contract writes it, or nothing.
fn stamp(value: Option<&Value>) -> Option<String> {
    let text = value?.as_str()?;
    let b = text.as_bytes();
    let shape = b.len() == 20
        && b[19] == b'Z'
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && b[..19]
            .iter()
            .enumerate()
            .all(|(i, c)| matches!(i, 4 | 7 | 10 | 13 | 16) || c.is_ascii_digit());
    shape.then(|| text.to_owned())
}

/// A name or key of the document, cut and cleaned (a person's own file, but still data).
fn text(value: Option<&Value>) -> Option<String> {
    let raw = value?.as_str()?;
    let clean: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME)
        .collect();
    (!clean.is_empty()).then_some(clean)
}

/// A count of the document: an integer, else unknown.
fn count(value: Option<&Value>) -> Option<u64> {
    value?.as_u64()
}

fn local_word(word: &str) -> Option<&'static str> {
    match word {
        "green" => Some("green"),
        "green-partial" => Some("green_partial"),
        "red" => Some("red"),
        _ => None,
    }
}

fn ci_local_word(word: &str) -> Option<&'static str> {
    match word {
        "error" => Some("error"),
        "infra" => Some("infra"),
        other => local_word(other),
    }
}

/// The states of `ci_remote` (the contract's list).
fn remote_word(word: &str) -> &'static str {
    match word {
        "failure" => "failure",
        "rejected" => "rejected",
        "cancelled" => "cancelled",
        "waiting_runner" => "waiting_runner",
        "queued" => "queued",
        "running" => "running",
        "success" => "success",
        "skipped" => "skipped",
        "no_run" => "no_run",
        "no_github" => "no_github",
        _ => "unknown",
    }
}

/// The light of one source; `None`: the source says nothing about how the repository stands.
fn local_light(word: &str, stale: Option<u64>) -> Light {
    match word {
        "red" => Light::Red,
        "green_partial" => Light::Amber,
        "green" if stale.is_some_and(|n| n > 0) => Light::Amber,
        _ => Light::Green,
    }
}

fn ci_local_light(word: &str) -> Light {
    match word {
        "red" => Light::Red,
        "green" => Light::Green,
        _ => Light::Amber,
    }
}

fn remote_light(word: &str) -> Option<Light> {
    match word {
        "failure" | "rejected" => Some(Light::Red),
        "success" => Some(Light::Green),
        "cancelled" | "unknown" | "waiting_runner" | "queued" | "running" => Some(Light::Amber),
        // No run, no GitHub, skipped: nothing to say.
        _ => None,
    }
}

fn row(value: &Value) -> Option<RepoLight> {
    let key = text(value.get("key"))?;
    let name = text(value.get("name"))?;
    let local_doc = value.get("local").filter(|l| l.is_object());
    let word = local_doc
        .and_then(|l| l.get("verdict"))
        .and_then(Value::as_str)
        .and_then(local_word);
    let stale = local_doc.and_then(|l| count(l.get("stale")));
    let ci_local = value
        .get("ci_local")
        .filter(|c| c.is_object())
        .and_then(|c| c.get("verdict"))
        .and_then(Value::as_str)
        .and_then(ci_local_word);
    let ci_remote = value
        .get("ci_remote")
        .filter(|c| c.is_object())
        .map(|c| remote_word(c.get("state").and_then(Value::as_str).unwrap_or("")));

    let lights = [
        word.map(|w| local_light(w, stale)),
        ci_local.map(ci_local_light),
        ci_remote.and_then(remote_light),
    ];
    let light = lights.into_iter().flatten().min().unwrap_or(Light::Unknown);
    Some(RepoLight {
        key,
        name,
        light,
        local: word,
        fail: value.get("counts").and_then(|c| count(c.get("fail"))),
        stale,
        ci_local,
        ci_remote,
        ts: local_doc.and_then(|l| stamp(l.get("ts"))),
    })
}

#[cfg(test)]
mod tests {
    use appkit_scratch::Scratch;
    use serde_json::json;

    use super::*;

    fn manifest(bytes: usize) -> String {
        json!({
            "schema": "testing-export/1", "kind": "manifest", "form": "export",
            "files": [{ "path": "fleet.json", "sha256": "0".repeat(64), "bytes": bytes }]
        })
        .to_string()
    }

    fn fleet(rows: Value) -> String {
        json!({
            "schema": "testing-export/1", "kind": "fleet", "form": "export",
            "generated_at": "2026-11-01T10:00:00Z",
            "classes": { "checked": true, "problem": null },
            "repos": rows
        })
        .to_string()
    }

    /// A folder with a fleet and the manifest that lists it with its real size.
    fn folder(fleet_text: &str) -> Scratch {
        let dir = Scratch::new("hub-monitor");
        std::fs::write(dir.path().join(FLEET), fleet_text).unwrap();
        std::fs::write(dir.path().join(MANIFEST), manifest(fleet_text.len())).unwrap();
        dir
    }

    fn read(dir: &Scratch) -> Result<Reading, &'static str> {
        read_dir(dir.path())
    }

    fn ready(reading: Reading) -> Vec<RepoLight> {
        match reading {
            Reading::Ready { repos, .. } => repos,
            other => panic!("not ready: {other:?}"),
        }
    }

    fn one(row: Value) -> RepoLight {
        let dir = folder(&fleet(json!([row])));
        ready(read(&dir).unwrap()).remove(0)
    }

    fn named(extra: Value) -> Value {
        let mut base = json!({ "key": "a-0123456789ab", "name": "a" });
        base.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        base
    }

    #[test]
    fn no_folder_is_not_configured() {
        assert_eq!(Monitor::new(None).read(), Reading::NotConfigured);
    }

    #[test]
    fn a_green_repository_is_green_and_a_red_case_makes_it_red() {
        let green = one(named(json!({
            "local": { "verdict": "green", "stale": 0, "ts": "2026-11-01T09:00:00Z" },
            "counts": { "fail": 0, "flaky": 0 }
        })));
        assert_eq!(green.light, Light::Green);
        assert_eq!(green.ts.as_deref(), Some("2026-11-01T09:00:00Z"));
        let red = one(named(json!({
            "local": { "verdict": "red", "stale": 0 },
            "counts": { "fail": 3, "flaky": 0 }
        })));
        assert_eq!((red.light, red.fail), (Light::Red, Some(3)));
    }

    #[test]
    fn the_worst_source_wins() {
        let r = one(named(json!({
            "local": { "verdict": "green" },
            "ci_remote": { "state": "failure" }
        })));
        assert_eq!(
            (r.light, r.local, r.ci_remote),
            (Light::Red, Some("green"), Some("failure"))
        );
        let amber = one(named(json!({
            "local": { "verdict": "green" },
            "ci_remote": { "state": "running" }
        })));
        assert_eq!(amber.light, Light::Amber);
        let ci = one(named(json!({ "ci_local": { "verdict": "infra" } })));
        assert_eq!((ci.light, ci.ci_local), (Light::Amber, Some("infra")));
    }

    #[test]
    fn outdated_or_partial_is_amber_and_nothing_is_unknown_never_green() {
        assert_eq!(
            one(named(
                json!({ "local": { "verdict": "green", "stale": 2 } })
            ))
            .light,
            Light::Amber
        );
        assert_eq!(
            one(named(json!({ "local": { "verdict": "green-partial" } }))).light,
            Light::Amber
        );
        let none = one(named(
            json!({ "local": null, "ci_local": null, "ci_remote": null }),
        ));
        assert_eq!(none.light, Light::Unknown);
        assert_eq!((none.local, none.fail, none.stale), (None, None, None));
        // "No run" and "no GitHub" say nothing; they do not make a repository green.
        assert_eq!(
            one(named(json!({ "ci_remote": { "state": "no_run" } }))).light,
            Light::Unknown
        );
    }

    #[test]
    fn a_word_nobody_knows_is_unknown_and_never_reaches_the_page() {
        let r = one(named(json!({
            "local": { "verdict": "<script>" },
            "ci_remote": { "state": "<script>" },
            "ci_local": { "verdict": "evil" }
        })));
        assert_eq!(
            (r.local, r.ci_local, r.ci_remote),
            (None, None, Some("unknown"))
        );
        assert_eq!(
            r.light,
            Light::Amber,
            "an unknown remote state is a reason to look"
        );
    }

    #[test]
    fn red_first_then_by_name() {
        let dir = folder(&fleet(json!([
            { "key": "b-1", "name": "b", "local": { "verdict": "green" } },
            { "key": "a-1", "name": "a", "local": { "verdict": "green" } },
            { "key": "c-1", "name": "c", "local": { "verdict": "red" } },
        ])));
        let names: Vec<_> = ready(read(&dir).unwrap())
            .into_iter()
            .map(|r| r.name)
            .collect();
        assert_eq!(names, ["c", "a", "b"]);
    }

    #[test]
    fn unknown_fields_are_ignored_and_a_row_without_a_name_is_dropped() {
        let dir = folder(&fleet(json!([
            { "key": "a-1", "name": "a", "future": { "x": 1 } },
            { "key": "b-1" },
            { "name": "c" },
            7
        ])));
        assert_eq!(ready(read(&dir).unwrap()).len(), 1);
    }

    #[test]
    fn a_key_that_is_repeated_is_shown_once() {
        let dir = folder(&fleet(json!([
            { "key": "a-1", "name": "a" },
            { "key": "a-1", "name": "a again" },
            { "key": "b-1", "name": "b" },
        ])));
        let names: Vec<_> = ready(read(&dir).unwrap())
            .into_iter()
            .map(|r| r.name)
            .collect();
        assert_eq!(names, ["a", "b"]);
    }

    #[test]
    fn texts_are_cut_and_cleaned() {
        let long = "x".repeat(500);
        let r = one(json!({ "key": "k-1", "name": format!("a\u{7}b\n{long}") }));
        assert!(r.name.starts_with("ab") && r.name.chars().count() == MAX_NAME);
    }

    #[test]
    fn a_missing_folder_or_manifest_says_so_without_a_path() {
        let dir = Scratch::new("hub-monitor-empty");
        assert_eq!(read_dir(dir.path()), Err(why::MISSING));
        let reading = Monitor::new(Some(dir.path().join("absent"))).read();
        assert_eq!(
            reading,
            Reading::Problem {
                problem: why::MISSING
            }
        );
    }

    #[test]
    fn a_higher_schema_is_refused_and_a_lower_minor_is_not() {
        let dir = folder(&fleet(json!([])));
        let newer = manifest(0).replace("testing-export/1", "testing-export/2");
        std::fs::write(dir.path().join(MANIFEST), newer).unwrap();
        assert_eq!(read(&dir), Err(why::TOO_NEW));
        let newer_fleet = fleet(json!([])).replace("testing-export/1", "testing-export/9");
        let dir = folder(&newer_fleet);
        assert_eq!(read(&dir), Err(why::TOO_NEW));
    }

    #[test]
    fn what_is_not_a_manifest_or_a_fleet_of_the_export_form_is_refused() {
        let dir = folder(&fleet(json!([])));
        for bad in [
            "{}",
            "[]",
            r#"{"schema":"other/1","kind":"manifest"}"#,
            r#"{"schema":"testing-export/1","kind":"fleet"}"#,
        ] {
            std::fs::write(dir.path().join(MANIFEST), bad).unwrap();
            assert_eq!(read(&dir), Err(why::NOT_AN_EXPORT), "{bad}");
        }
        std::fs::write(dir.path().join(MANIFEST), "nope").unwrap();
        assert_eq!(read(&dir), Err(why::NOT_JSON));

        let local = fleet(json!([])).replace(r#""form":"export""#, r#""form":"local""#);
        assert_eq!(read(&folder(&local)), Err(why::NOT_A_FLEET));
        let repo = fleet(json!([])).replace(r#""kind":"fleet""#, r#""kind":"repo""#);
        assert_eq!(read(&folder(&repo)), Err(why::NOT_A_FLEET));
    }

    #[test]
    fn a_fleet_made_without_the_data_classes_is_refused() {
        for text in [
            fleet(json!([])).replace(r#""checked":true"#, r#""checked":false"#),
            json!({ "schema": "testing-export/1", "kind": "fleet", "form": "export", "repos": [] })
                .to_string(),
        ] {
            assert_eq!(read(&folder(&text)), Err(why::CLASSES));
        }
    }

    #[test]
    fn a_fleet_of_another_size_than_the_manifest_says_is_refused() {
        let dir = folder(&fleet(json!([])));
        std::fs::write(dir.path().join(MANIFEST), manifest(1)).unwrap();
        assert_eq!(read(&dir), Err(why::SIZE));
        let mut without: Value = serde_json::from_str(&manifest(0)).unwrap();
        without["files"] = json!([{ "path": "other.json", "bytes": 1 }]);
        std::fs::write(dir.path().join(MANIFEST), without.to_string()).unwrap();
        assert_eq!(read(&dir), Err(why::NO_FLEET));
    }

    #[test]
    fn a_document_bigger_than_the_limit_is_not_read() {
        let dir = folder(&fleet(json!([])));
        let big = std::fs::File::create(dir.path().join(FLEET)).unwrap();
        big.set_len(MAX_BYTES + 1).unwrap();
        assert_eq!(read(&dir), Err(why::TOO_BIG));
    }

    #[test]
    fn the_workstation_export_says_the_local_columns_are_unknown() {
        let text = fleet(json!([{ "key": "a-1", "name": "a", "local": null,
            "ci_remote": { "state": "success" } }]))
        .replace(r#""kind":"fleet""#, r#""kind":"fleet","local_read":false"#);
        match read(&folder(&text)).unwrap() {
            Reading::Ready {
                local_read, repos, ..
            } => {
                assert!(!local_read);
                assert_eq!(repos[0].light, Light::Green);
            }
            other => panic!("{other:?}"),
        }
    }

    fn repo_doc(key: &str, cases: Value) -> String {
        json!({
            "schema": "testing-export/1", "kind": "repo", "form": "export", "local_read": true,
            "repo": { "key": key, "name": "a" },
            "cases_red": 2, "cases_omitted": 0,
            "cases": cases
        })
        .to_string()
    }

    /// A folder with one repository document, listed by the manifest with `bytes` (its real size by default).
    fn repo_folder(key: &str, doc: &str, bytes: Option<usize>) -> Scratch {
        let dir = Scratch::new("hub-cases");
        std::fs::create_dir_all(dir.path().join("repos")).unwrap();
        std::fs::write(dir.path().join("repos").join(format!("{key}.json")), doc).unwrap();
        let manifest = json!({
            "schema": "testing-export/1", "kind": "manifest", "form": "export",
            "files": [{ "path": format!("repos/{key}.json"), "sha256": "0".repeat(64), "bytes": bytes.unwrap_or(doc.len()) }]
        });
        std::fs::write(dir.path().join(MANIFEST), manifest.to_string()).unwrap();
        dir
    }

    fn red(file: &str, line: u64, status: &str) -> Value {
        json!({
            "id": format!("{file}::x"), "file": file, "line": line, "status": status, "name": "reads a file",
            "describe": ["parser"], "duration_ms": 4, "flaky": false,
            "message": "expected\n\tb", "expected": "b", "actual": null, "fails": [], "fails_total": null
        })
    }

    fn cases_of(dir: &Scratch, key: &str) -> CasesReading {
        Monitor::new(Some(dir.path().to_path_buf())).cases(key)
    }

    #[test]
    fn the_red_cases_of_a_repository_come_by_file_and_line_with_their_texts() {
        let key = "a-0123456789ab";
        let doc = repo_doc(
            key,
            json!([
                red("b_spec.lua", 3, "timeout"),
                red("a_spec.lua", 9, "fail"),
                red("a_spec.lua", 2, "crash")
            ]),
        );
        let dir = repo_folder(key, &doc, None);
        let CasesReading::Ready {
            cases,
            cases_red,
            cases_omitted,
            local_read,
            ..
        } = cases_of(&dir, key)
        else {
            panic!("not ready");
        };
        assert!(local_read);
        assert_eq!((cases_red, cases_omitted), (Some(2), Some(0)));
        let at: Vec<_> = cases
            .iter()
            .map(|c| (c.file.as_str(), c.line, c.status))
            .collect();
        assert_eq!(
            at,
            [
                ("a_spec.lua", Some(2), "crash"),
                ("a_spec.lua", Some(9), "fail"),
                ("b_spec.lua", Some(3), "timeout")
            ]
        );
        // Line breaks and tabs of a failure text stay, as text.
        assert_eq!(cases[0].message.as_deref(), Some("expected\n\tb"));
        assert_eq!(cases[0].describe, ["parser"]);
    }

    #[test]
    fn a_case_that_is_not_red_or_has_no_file_is_left_out_and_texts_are_cut() {
        let key = "a-0123456789ab";
        let mut long = red("x_spec.lua", 1, "fail");
        long["message"] = json!(format!("a\u{1b}[31m{}", "m".repeat(5000)));
        long["name"] = json!("n\r\u{7}".repeat(200));
        let doc = repo_doc(
            key,
            json!([
                red("p_spec.lua", 1, "pass"),
                red("s_spec.lua", 1, "skip"),
                red("w_spec.lua", 1, "weird"),
                json!({ "status": "fail", "line": 1 }),
                long
            ]),
        );
        let dir = repo_folder(key, &doc, None);
        let CasesReading::Ready { cases, .. } = cases_of(&dir, key) else {
            panic!("not ready")
        };
        assert_eq!(cases.len(), 1);
        let message = cases[0].message.as_deref().unwrap();
        assert_eq!(message.chars().count(), MAX_TEXT);
        assert!(!message.contains('\u{1b}'));
        assert!(!cases[0].name.chars().any(char::is_control));
        assert!(cases[0].name.chars().count() <= MAX_LABEL);
    }

    #[test]
    fn more_cases_than_the_hub_shows_are_counted_as_left_out() {
        let key = "a-0123456789ab";
        let many: Vec<Value> = (1..=(MAX_CASES as u64 + 5))
            .map(|n| red("x_spec.lua", n, "fail"))
            .collect();
        let dir = repo_folder(key, &repo_doc(key, json!(many)), None);
        let CasesReading::Ready {
            cases,
            cases_omitted,
            ..
        } = cases_of(&dir, key)
        else {
            panic!("not ready")
        };
        assert_eq!(cases.len(), MAX_CASES);
        assert_eq!(cases_omitted, Some(5));
    }

    #[test]
    fn only_a_listed_document_of_this_key_and_the_listed_size_is_read() {
        let key = "a-0123456789ab";
        let doc = repo_doc(key, json!([red("x_spec.lua", 1, "fail")]));
        // Another size than the manifest says.
        let dir = repo_folder(key, &doc, Some(doc.len() + 1));
        assert_eq!(
            cases_of(&dir, key),
            CasesReading::Problem {
                problem: why::REPO_SIZE
            }
        );
        // A document of another repository under this name.
        let other = repo_doc("b-0123456789ab", json!([]));
        let dir = repo_folder(key, &other, None);
        assert_eq!(
            cases_of(&dir, key),
            CasesReading::Problem {
                problem: why::NOT_A_REPO
            }
        );
        // A key the manifest does not list, also when the file is there.
        let dir = repo_folder(key, &doc, None);
        std::fs::write(dir.path().join("repos").join("c-0123456789ab.json"), &doc).unwrap();
        assert_eq!(
            cases_of(&dir, "c-0123456789ab"),
            CasesReading::Problem {
                problem: why::NO_REPO
            }
        );
        // No folder named.
        assert_eq!(Monitor::new(None).cases(key), CasesReading::NotConfigured);
    }

    #[test]
    fn a_key_that_is_no_plain_name_is_never_joined_to_a_path() {
        let key = "a-0123456789ab";
        let dir = repo_folder(key, &repo_doc(key, json!([])), None);
        for bad in [
            "",
            "../a-0123456789ab",
            "a/b",
            r"a\b",
            ".hidden",
            "a b",
            "a%2e",
            &"k".repeat(121),
        ] {
            assert_eq!(
                cases_of(&dir, bad),
                CasesReading::Problem {
                    problem: why::NO_REPO
                },
                "{bad:?}"
            );
        }
    }

    /// A manifest made by hand may list a path that leaves `repos/`: the key is checked before any path is made.
    #[test]
    fn a_manifest_that_lists_a_path_out_of_repos_is_not_followed() {
        let key = "../secret";
        let dir = Scratch::new("hub-cases");
        std::fs::create_dir_all(dir.path().join("repos")).unwrap();
        let doc = repo_doc(key, json!([red("x_spec.lua", 1, "fail")]));
        std::fs::write(dir.path().join("secret.json"), &doc).unwrap();
        let manifest = json!({
            "schema": "testing-export/1", "kind": "manifest", "form": "export",
            "files": [{ "path": "repos/../secret.json", "sha256": "0".repeat(64), "bytes": doc.len() }]
        });
        std::fs::write(dir.path().join(MANIFEST), manifest.to_string()).unwrap();
        assert_eq!(
            cases_of(&dir, key),
            CasesReading::Problem {
                problem: why::NO_REPO
            }
        );
    }

    #[test]
    fn a_document_of_another_kind_form_or_schema_is_refused() {
        let key = "a-0123456789ab";
        for (field, value, want) in [
            ("kind", json!("fleet"), why::NOT_A_REPO),
            ("form", json!("local"), why::NOT_A_REPO),
            ("schema", json!("testing-export/2"), why::TOO_NEW),
        ] {
            let mut doc: Value = serde_json::from_str(&repo_doc(key, json!([]))).unwrap();
            doc[field] = value;
            let dir = repo_folder(key, &doc.to_string(), None);
            assert_eq!(
                cases_of(&dir, key),
                CasesReading::Problem { problem: want },
                "{field}"
            );
        }
    }

    #[test]
    fn without_a_local_read_there_are_no_cases_and_the_counts_are_unknown() {
        let key = "a-0123456789ab";
        let mut doc: Value =
            serde_json::from_str(&repo_doc(key, json!([red("x_spec.lua", 1, "fail")]))).unwrap();
        doc["local_read"] = json!(false);
        let dir = repo_folder(key, &doc.to_string(), None);
        assert_eq!(
            cases_of(&dir, key),
            CasesReading::Ready {
                key: key.to_owned(),
                local_read: false,
                cases_red: None,
                cases_omitted: None,
                cases: Vec::new()
            }
        );
    }

    /// The red cases of the golden export: beta has two, by file and line.
    #[test]
    fn the_red_cases_of_the_golden_export_are_read() {
        let golden = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../testing.nvim/TESTS/golden/testing-export-1");
        if !golden.join(MANIFEST).is_file() {
            return;
        }
        let repos = ready(read_dir(&golden).expect("the golden export is readable"));
        let beta = repos.iter().find(|r| r.name == "beta.nvim").expect("beta");
        let CasesReading::Ready { cases, .. } = Monitor::new(Some(golden)).cases(&beta.key) else {
            panic!("not ready");
        };
        assert_eq!(cases.len(), 2);
        assert!(cases.iter().all(|c| !c.file.is_empty()));
    }

    /// The golden export of testing.nvim, if this machine has it next to the hub: the contract as it is written.
    #[test]
    fn the_golden_export_of_testing_nvim_is_read() {
        let golden = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../testing.nvim/TESTS/golden/testing-export-1");
        if !golden.join(MANIFEST).is_file() {
            return;
        }
        // The golden manifest lists a fleet of the size on disk (byte-stable files, see its README).
        let repos = ready(read_dir(&golden).expect("the golden export is readable"));
        assert!(!repos.is_empty());
        assert!(repos
            .iter()
            .all(|r| !r.key.is_empty() && !r.name.is_empty()));
    }
}
