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
