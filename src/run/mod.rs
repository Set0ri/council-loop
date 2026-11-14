//! The resumable Council Loop state machine (behind `council init / next / ingest / cut`).
//!
//! ELI5: a council run is a long checklist with many hands on it. This module keeps the checklist on
//! disk, gives every seat its instructions as a file, checks what comes back, and always knows the
//! next step. The chair is an AI with a finite memory: it can stop, forget, and pick the run up
//! again with `council status`.
//!
//! A run lives in `.council/runs/<id>/`:
//!
//! ```text
//! run.json                 the state, written only by this tool
//! brief.yaml               the chair's intent brief (rubric first)
//! facts.md                 optional facts ledger
//! control/                 the one-shot baseline (the chair does not read it until the end)
//! loop-N/<phase>/<seat>.prompt.md and <seat>.out.json     one pair per seat
//! loop-N/research/         the researchers (facts) and the mapper (MAP.out.md, the codebase map)
//! loop-N/pool.json, candidates.json                        the chair's merge and fuse steps
//! loop-N/ledger.md         written by `cut`
//! final.md, retro.md
//! ```
//!
//! Which seat wrote which blinded card is kept outside the project (`~/.council-private/`), so a seat
//! reading the run folder cannot stumble on it.
//!
//! The council also shares a memory that outlives the run ([`crate::memory`]): facts with sources,
//! and what members say to each other. Seats never write it. They put `research` and `notes` in
//! their output and this tool files them, so every note has one shape and none reaches a blind vote.

mod cut;
mod dispatch;
mod ingest;
mod prompts;
mod seats;
mod status;
mod steps;
mod validate;

pub use cut::cmd_cut;
pub use dispatch::cmd_dispatch;
pub use ingest::cmd_ingest;
pub use status::{cmd_audit, cmd_deviation, cmd_note, cmd_status};
pub use steps::{cmd_init, cmd_map_packet, cmd_next};

pub(crate) use ingest::*;
pub(crate) use prompts::*;
pub(crate) use seats::*;
pub(crate) use steps::*;
pub(crate) use validate::*;

pub(crate) use crate::core::{self, fail};
pub(crate) use crate::py::{self, Json};
pub(crate) use crate::val::{as_int, items, Get};
pub(crate) use crate::{codemap, memory, providers, say};
pub(crate) use anyhow::Result;
pub(crate) use indexmap::IndexMap;
pub(crate) use regex::Regex;
pub(crate) use serde_json::{json, Map, Value};
pub(crate) use std::collections::{BTreeMap, HashMap, HashSet};
pub(crate) use std::path::{Path, PathBuf};
pub(crate) use std::sync::LazyLock;

/// Template variables: placeholder name -> text.
pub(crate) type Vars = HashMap<&'static str, String>;

/// A card body under this many words cannot be judged fairly against a full one.
pub(crate) const BODY_MIN: usize = 60;
/// How much longer the longest blinded card may be than the shortest.
pub(crate) const SPREAD: f64 = 2.3;
/// Steps the chair does itself: no seat to drop.
pub(crate) const CHAIR_STEPS: [&str; 6] = ["direct", "brief", "merge", "fuse", "final", "retro"];

static STOP: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    "the a an and or of to in on for with by is are be as at it this that from into over via not no \
     each every per then than its their our your you we they them will can should must may"
        .split_whitespace()
        .collect()
});
static WORD_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[a-z0-9]+").expect("valid"));
/// A lesson line, as `docs/lessons.md` and a retro hold it.
pub(crate) static LESSON_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^- \[(L-[\w-]+)\] \(helpful: (\d+) \| harmful: (\d+)\) (.+)$").expect("valid")
});

/// The agent that plays a role.
pub(crate) fn agent(role: &str) -> &'static str {
    match role {
        "red_team" => "council-red-team",
        "elder" => "council-elder",
        "researcher" => "council-researcher",
        "mapper" => "council-mapper",
        _ => "council-expert",
    }
}

/// Researchers and the mapper bring facts and a map; they never propose or vote.
pub(crate) fn is_support(role: &str) -> bool {
    matches!(role, "researcher" | "mapper")
}

// ----------------------------------------------------------------------------- small helpers

/// The time now, as run files record it.
pub(crate) fn now() -> String {
    crate::clock::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}

/// A short name for a run, from the first meaningful words of the ask.
pub(crate) fn slug(text: &str, n: usize) -> String {
    let lower = text.to_lowercase();
    let words: Vec<&str> = WORD_RE
        .find_iter(&lower)
        .map(|m| m.as_str())
        .filter(|w| !STOP.contains(w) && w.len() > 2)
        .take(n)
        .collect();
    if words.is_empty() {
        "run".to_string()
    } else {
        words.join("-")
    }
}

/// The words of a text that tell two cards apart.
pub(crate) fn card_tokens(text: &str) -> HashSet<String> {
    let lower = text.to_lowercase();
    WORD_RE
        .find_iter(&lower)
        .map(|m| m.as_str())
        .filter(|w| w.len() > 3 && !STOP.contains(w))
        .map(str::to_string)
        .collect()
}

/// A path as the chair should see it: absolute.
pub(crate) fn show(p: &Path) -> String {
    core::resolve(p).display().to_string()
}

/// Fill a template from the skill's `templates/` folder.
pub(crate) fn tpl(name: &str, vars: &Vars) -> Result<String> {
    match crate::assets::read(&format!("templates/{name}")) {
        Some(text) => Ok(crate::tpl::substitute(&text, vars)),
        None => fail(format!(
            "the template {name} is missing from the skill folder"
        )),
    }
}

/// A text as a Markdown quote.
pub(crate) fn quote(text: &str) -> String {
    py::lines(py::strip(text))
        .iter()
        .map(|line| format!("> {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Where the runs of a project live.
pub(crate) fn runs_dir(root: &Path) -> PathBuf {
    root.join(".council").join("runs")
}

/// Load a run (the current one when no id is given): its state and its folder.
pub fn load(run_id: Option<&str>) -> Result<(Value, PathBuf)> {
    let base = runs_dir(&core::project_root());
    let id = match run_id.filter(|r| !r.is_empty()) {
        Some(id) => id.to_string(),
        None => {
            let current = base.join("CURRENT");
            if !current.exists() {
                return fail("no current run. Start one: council init \"<the ask, verbatim>\"");
            }
            py::strip(&core::read_text(&current)?).to_string()
        }
    };
    let d = base.join(&id);
    let file = d.join("run.json");
    if !file.exists() {
        return fail(format!("run not found: {id}"));
    }
    let run: Value = serde_json::from_str(&core::read_text(&file)?)
        .map_err(|e| anyhow::anyhow!("{} is not valid JSON: {e}", file.display()))?;
    Ok((run, d))
}

/// Write the run's state. The file is replaced in one step, so a crash never leaves half a state.
pub(crate) fn save(run: &mut Value, d: &Path) -> Result<()> {
    run["updated"] = json!(now());
    let tmp = d.join("run.json.tmp");
    core::write_text(&tmp, &Json::ONE.dumps(run))?;
    std::fs::rename(&tmp, d.join("run.json"))?;
    Ok(())
}

/// The file that holds what must stay away from the seats (outside the project when possible).
pub(crate) fn private_file(run: &Value, d: &Path) -> PathBuf {
    let base = match std::env::var_os("COUNCIL_PRIVATE_DIR").filter(|v| !v.is_empty()) {
        Some(dir) => py::plain(dir),
        None => core::home_dir().join(".council-private"),
    };
    match std::fs::create_dir_all(&base) {
        Ok(()) => base.join(format!("{}.json", run.s("id"))),
        Err(_) => d.join(".private.json"),
    }
}

/// What must stay away from the seats: which seat wrote which blinded card.
pub(crate) fn private(run: &Value, d: &Path) -> Value {
    let f = private_file(run, d);
    core::read_text(&f)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

pub(crate) fn save_private(run: &Value, d: &Path, data: &Value) -> Result<()> {
    core::write_text(&private_file(run, d), &Json::ONE.dumps(data))
}

/// SHA-256 of a JSON value in a canonical form (sorted keys), in hex.
pub(crate) fn sha_value(value: &Value) -> String {
    core::sha256_hex(Json::LINE.sorted().unicode().dumps(value).as_bytes())
}

/// The retry prompt that belongs to a seat's prompt file.
pub(crate) fn retry_file(prompt: &Path) -> PathBuf {
    let name = prompt
        .file_name()
        .map(|n| n.to_string_lossy().replace(".prompt.md", ".retry.md"))
        .unwrap_or_default();
    prompt.with_file_name(name)
}

/// The one-shot baseline is run once, next to the first ideation, unless it was already settled.
pub(crate) fn wants_control(run: &Value) -> bool {
    run.int("loop") == Some(1) && !run.has("control")
}

/// The seed for a blinded packet: the same run, loop and step always shuffle the same way.
pub(crate) fn seed_for(run: &Value, phase: &str) -> u64 {
    u64::from(py::crc32(
        format!("{}/{}/{phase}", run.s("id"), run.show("loop")).as_bytes(),
    ))
}

/// The seat ids that hold a role, in order.
pub(crate) fn seats_of(run: &Value, role: &str) -> Vec<String> {
    let mut out: Vec<String> = items(run.at("seats"))
        .filter(|(_, v)| v.s("role") == role)
        .map(|(s, _)| s.clone())
        .collect();
    out.sort();
    out
}

/// The loop the run is in (1-based; 0 before the brief is accepted).
pub(crate) fn loop_no(run: &Value) -> usize {
    run.int("loop").unwrap_or(0).max(0) as usize
}

/// The record of the current loop.
pub(crate) fn cur(run: &Value) -> &Value {
    let loops = run.list("loops");
    let index = loop_no(run)
        .saturating_sub(1)
        .min(loops.len().saturating_sub(1));
    loops.get(index).unwrap_or(&Value::Null)
}

/// The record of the current loop, to change it.
pub(crate) fn cur_mut(run: &mut Value) -> &mut Value {
    let index = loop_no(run).saturating_sub(1);
    if !run["loops"].is_array() {
        run["loops"] = json!([]);
    }
    let loops = run["loops"].as_array_mut().expect("loops is a list");
    while loops.len() <= index {
        loops.push(json!({}));
    }
    &mut loops[index]
}

/// The folder of the current loop, created on demand.
pub(crate) fn loop_dir(run: &Value, d: &Path) -> Result<PathBuf> {
    let p = d.join(format!("loop-{}", run.show("loop")));
    std::fs::create_dir_all(&p)?;
    Ok(p)
}

/// How many candidates this loop must end with before the cut.
pub(crate) fn n_fuse(run: &Value) -> usize {
    run.list("schedule")
        .get(loop_no(run).saturating_sub(1))
        .and_then(Value::as_u64)
        .unwrap_or(1) as usize
}

/// Is this the last loop the run may use?
pub(crate) fn is_last(run: &Value) -> bool {
    run.int("loop").unwrap_or(0) >= run.int("max_loops").unwrap_or(0)
}

/// The ids of the rubric's criteria.
pub(crate) fn rubric_ids(run: &Value) -> Vec<String> {
    run.at("brief")
        .list("rubric")
        .iter()
        .map(|r| r.show("id"))
        .collect()
}

/// The rubric as a Markdown table.
pub(crate) fn rubric_table(run: &Value) -> String {
    let mut rows = vec![
        "| id | criterion | weight | checked by |".to_string(),
        "| --- | --- | --- | --- |".to_string(),
    ];
    for r in run.at("brief").list("rubric") {
        let check = r
            .get("check")
            .map(py::to_str)
            .unwrap_or_else(|| "judgment".to_string());
        rows.push(format!(
            "| {} | {} | {} | {check} |",
            r.show("id"),
            r.show("name"),
            r.show("weight")
        ));
    }
    rows.join("\n")
}

/// The facts the brief gives every seat, plus `facts.md` when it is short enough to inline.
pub(crate) fn facts_block(run: &Value, d: &Path) -> String {
    let brief = run.at("brief");
    let mut text = if brief.truthy("facts") {
        py::strip(&brief.show("facts")).to_string()
    } else {
        String::new()
    };
    let f = d.join("facts.md");
    if f.exists() {
        let body = core::read_text(&f).unwrap_or_default();
        let body = py::strip(&body);
        if py::words(body) <= 600 {
            text.push('\n');
            text.push_str(body);
        } else {
            text.push_str(&format!("\nMore facts: read {}", show(&f)));
        }
    }
    let text = py::strip(&text);
    if text.is_empty() {
        String::new()
    } else {
        format!("\n## Facts\n{text}\n")
    }
}

/// Lessons from earlier runs that have helped at least as often as they hurt, most helpful first.
pub(crate) fn lessons(root: &Path) -> Vec<Value> {
    let f = root.join("docs").join("lessons.md");
    let mut out: Vec<(i64, Value)> = Vec::new();
    if let Ok(text) = core::read_text(&f) {
        for line in py::lines(&text) {
            if let Some(m) = LESSON_RE.captures(py::strip(line)) {
                let (helpful, harmful): (i64, i64) =
                    (m[2].parse().unwrap_or(0), m[3].parse().unwrap_or(0));
                if helpful >= harmful {
                    out.push((
                        harmful - helpful,
                        json!({"id": &m[1], "helpful": helpful, "harmful": harmful, "text": &m[4]}),
                    ));
                }
            }
        }
    }
    out.sort_by_key(|(key, _)| *key);
    out.into_iter().map(|(_, lesson)| lesson).collect()
}

/// `str(value or "")` with the ends trimmed: the text of a field a model or a person filled in.
pub(crate) fn text_of(value: &Value) -> String {
    if py::truthy(value) {
        py::strip(&py::to_str(value)).to_string()
    } else {
        String::new()
    }
}

/// `council.words(value)`: the number of words in a field, 0 when it is empty or missing.
pub(crate) fn words_of(value: &Value) -> usize {
    if py::truthy(value) {
        py::words(&py::to_str(value))
    } else {
        0
    }
}

/// A list the way Python prints one inside a message: `['A', 'B']`.
pub(crate) fn listed<T: serde::Serialize>(items: &[T]) -> String {
    py::repr(&json!(items))
}

/// A whole number or a float (Python's `isinstance(x, (int, float))`, which a boolean passes too).
pub(crate) fn is_number(value: &Value) -> bool {
    value.is_number() || value.is_boolean()
}
