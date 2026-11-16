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

pub use status::{cmd_audit, cmd_log, cmd_status, snapshot};
pub use steps::cmd_map_packet;

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

/// The agent that plays a seat. A researcher gets the web (`council-researcher`) or, when its beat
/// is the project's own record, the project's files (`council-historian`): never both in one seat.
pub(crate) fn agent(seat: &Value) -> &'static str {
    match seat.s("role") {
        "red_team" => "council-red-team",
        "elder" => "council-elder",
        "researcher" if seat.s("beat") == core::BEATS[3].0 => "council-historian",
        "researcher" => "council-researcher",
        "mapper" => "council-mapper",
        _ => "council-expert",
    }
}

/// Researchers and the mapper bring facts and a map; they never propose or vote.
pub(crate) fn is_support(role: &str) -> bool {
    matches!(role, "researcher" | "mapper")
}

// ----------------------------------------------------------------------------- the verbs, with a diary

/// Where a run stands (id, step, loop, folder), read without judging it.
fn peek(run_id: Option<&str>) -> Option<(String, String, i64, PathBuf)> {
    let (run, d) = load(run_id).ok()?;
    Some((
        run.s("id").to_string(),
        run.s("phase").to_string(),
        run.int("loop").unwrap_or(0),
        d,
    ))
}

/// Take one step of the loop and write it into the run's diary (`events.jsonl`): the verb, where
/// the run stood before and after, how it went and how long it took.
fn logged(verb: &str, run_id: Option<&str>, work: impl FnOnce() -> Result<i32>) -> Result<i32> {
    let before = if verb == "init" { None } else { peek(run_id) };
    let started = std::time::Instant::now();
    let result = work();
    if verb == "init" && result.is_err() {
        return result; // no run was started: there is no diary to write in
    }
    let ms = crate::log::took(started);
    let id = run_id
        .map(str::to_string)
        .or_else(|| before.as_ref().map(|b| b.0.clone()));
    if let Some((id, phase, at, d)) = peek(id.as_deref()) {
        let (ok, code, error) = match &result {
            Ok(code) => (*code == 0, *code, Value::Null),
            Err(err) => {
                let user = err.downcast_ref::<core::UserError>().is_some();
                // the first line says what is wrong; the lines after it repeat the step's
                // instructions (with paths), which the diary does not need
                let said = format!("{err:#}");
                (
                    false,
                    if user { 2 } else { 1 },
                    json!(py::head(py::strip(said.lines().next().unwrap_or("")), 200)),
                )
            }
        };
        let was = |pick: fn(&(String, String, i64, PathBuf)) -> Value| {
            before.as_ref().map(pick).unwrap_or(Value::Null)
        };
        crate::diag!(Info, "run", "{verb}: {} -> {phase}", before.as_ref().map_or("-", |b| b.1.as_str());
                     "run" => id, "ok" => ok, "ms" => ms);
        crate::log::record(
            &d,
            json!({"event": "verb", "verb": verb, "ok": ok, "code": code, "ms": ms,
                   "phase": [was(|b| json!(b.1)), phase], "loop": [was(|b| json!(b.2)), at], "error": error}),
        );
    }
    result
}

/// `council init`: run the gate, seat the council and write the run's first state.
pub fn cmd_init(ask: &str, args: &crate::gate::GateArgs, obsidian: bool) -> Result<i32> {
    logged("init", None, || steps::cmd_init(ask, args, obsidian))
}

/// `council next`: prepare the current step and say what to do.
pub fn cmd_next(run_id: Option<&str>, without_control: Option<&str>) -> Result<i32> {
    logged("next", run_id, || steps::cmd_next(run_id, without_control))
}

/// `council dispatch`: send every CLI or API seat of this step its prompt and save what comes back.
pub fn cmd_dispatch(run_id: Option<&str>, seat: Option<&str>, dry_run: bool) -> Result<i32> {
    if dry_run {
        return dispatch::cmd_dispatch(run_id, seat, dry_run); // nothing happened: nothing to record
    }
    logged("dispatch", run_id, || {
        dispatch::cmd_dispatch(run_id, seat, dry_run)
    })
}

/// `council ingest`: validate what came back; advance when it is all valid.
pub fn cmd_ingest(run_id: Option<&str>, drop: Option<&str>, reason: Option<&str>) -> Result<i32> {
    logged("ingest", run_id, || {
        ingest::cmd_ingest(run_id, drop, reason)
    })
}

/// `council cut`: tally, elders and wildcard decide the survivors; writes the loop's ledger.
pub fn cmd_cut(
    run_id: Option<&str>,
    eli5: &str,
    changed: &str,
    stop: Option<&str>,
    asked: Option<&str>,
    unattended: Option<&str>,
) -> Result<i32> {
    logged("cut", run_id, || {
        cut::cmd_cut(run_id, eli5, changed, stop, asked, unattended)
    })
}

/// `council deviation`: record that the run left the protocol, and why.
pub fn cmd_deviation(run_id: Option<&str>, text: &str) -> Result<i32> {
    logged("deviation", run_id, || status::cmd_deviation(run_id, text))
}

/// `council note`: post to the run's commons.
pub fn cmd_note(run_id: Option<&str>, text: &str, to: &str) -> Result<i32> {
    logged("note", run_id, || status::cmd_note(run_id, text, to))
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

/// The steps a run can be at.
pub(crate) const PHASE_NAMES: [&str; 14] = [
    "direct", "brief", "research", "ideate", "merge", "vote", "redteam", "fuse", "rank", "elders",
    "cut", "final", "retro", "done",
];

/// A run id is a name, never a path: a letter or digit, then letters, digits, `.`, `_` or `-`.
pub(crate) fn is_run_id(id: &str) -> bool {
    let mut chars = id.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && id.len() <= 128
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// A seat id: a capital letter, then up to seven capitals or digits (`L3`, `RT`, `EB`, `MAP`).
pub(crate) fn is_seat_id(id: &str) -> bool {
    let mut chars = id.chars();
    chars.next().is_some_and(|c| c.is_ascii_uppercase())
        && id.len() <= 8
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// A run file is this tool's own record, but it lies in the project, and a project can come from
/// anyone. Before a run is used, its file must look as this tool writes it, and the memory folder
/// it names must be one this tool may use.
fn check_state(run: &Value, d: &Path) -> Result<()> {
    let bad = |why: String| -> Result<()> {
        fail(format!(
            "{} is not a run file as this tool writes it: {why}",
            show(&d.join("run.json"))
        ))
    };
    if !run.is_object() {
        return bad("it is not a JSON object".to_string());
    }
    let whole = |v: &Value| v.is_i64() || v.is_u64();
    type Check = (&'static str, fn(&Value) -> bool);
    let every: [Check; 8] = [
        ("id", Value::is_string),
        ("ask", Value::is_string),
        ("gate", Value::is_string),
        ("tier", Value::is_string),
        ("phase", Value::is_string),
        ("calls", Value::is_object),
        ("loops", Value::is_array),
        ("deviations", Value::is_array),
    ];
    #[allow(clippy::type_complexity)]
    let council: [(&str, fn(&Value) -> bool); 9] = [
        ("seats", Value::is_object),
        ("loop", whole),
        ("max_loops", whole),
        ("schedule", Value::is_array),
        ("shape", Value::is_object),
        ("support", Value::is_object),
        ("memory", Value::is_object),
        ("chair", |v| v.is_object() || v.is_string()), // solo mode: "current model @ medium"
        ("commons", Value::is_array),
    ];
    let direct = run.s("tier") == "direct";
    for (key, fits) in every.iter().chain(council.iter().filter(|_| !direct)) {
        if !fits(run.at(key)) {
            return bad(format!("`{key}` is missing or of the wrong kind"));
        }
    }
    if d.file_name().and_then(|n| n.to_str()) != Some(run.s("id")) {
        return bad("its id is not the name of its folder".to_string());
    }
    if !core::TIERS.contains(&run.s("tier")) || !PHASE_NAMES.contains(&run.s("phase")) {
        return bad("its tier or its phase is not one this tool knows".to_string());
    }
    if direct {
        return Ok(());
    }
    let (at, cap) = (
        run.int("loop").unwrap_or(-1),
        run.int("max_loops").unwrap_or(0),
    );
    if at < 0 || at as usize > run.list("loops").len() || !(1..=99).contains(&cap) {
        return bad("its loop counter does not fit its loops".to_string());
    }
    for (seat, v) in items(run.at("seats")) {
        let named = crate::safe::shown(py::head(seat, 20)).into_owned();
        if !is_seat_id(seat) || !v.is_object() || !v.at("role").is_string() {
            return bad(format!("seat {} is not a seat", py::str_repr(&named)));
        }
        // these two are handed to another program as option values
        for key in ["native_id", "effort"] {
            if v.at(key)
                .as_str()
                .is_some_and(|text| !crate::safe::plain_arg(text))
            {
                return bad(format!(
                    "seat {} names a {key} that is not one",
                    py::str_repr(&named)
                ));
            }
        }
    }
    if let Some(why) = memory::trusted(run.at("memory"), None) {
        return fail(why);
    }
    Ok(())
}

/// Load a run (the current one when no id is given): its state and its folder.
pub fn load(run_id: Option<&str>) -> Result<(Value, PathBuf)> {
    let base = runs_dir(&core::project_root());
    let id = match run_id.filter(|r| !r.is_empty()) {
        Some(id) => id.to_string(),
        None => {
            let current = base.join("CURRENT");
            if !current.exists() && !crate::safe::is_link(&current) {
                return fail("no current run. Start one: council init \"<the ask, verbatim>\"");
            }
            py::strip(&crate::safe::read_capped(&current, 4096)?).to_string()
        }
    };
    if !is_run_id(&id) {
        // a name, never a path: CURRENT lies in the project
        return fail(format!(
            "run not found: {} is not a run id",
            py::str_repr(&crate::safe::shown(py::head(&id, 80)))
        ));
    }
    let d = base.join(&id);
    let file = d.join("run.json");
    if crate::safe::plain_file(&file, crate::safe::MAX_READ).is_none() {
        return fail(format!("run not found: {id}"));
    }
    let run: Value = serde_json::from_str(&crate::safe::read_own(&file)?)
        .map_err(|e| anyhow::anyhow!("{} is not valid JSON: {e}", file.display()))?;
    check_state(&run, &d)?;
    Ok((run, d))
}

/// Write the run's state. The file is replaced in one step, so a crash never leaves half a state.
pub(crate) fn save(run: &mut Value, d: &Path) -> Result<()> {
    run["updated"] = json!(now());
    let tmp = d.join("run.json.tmp");
    core::write_text(&tmp, &Json::ONE.dumps(run))?;
    crate::safe::rename(&tmp, &d.join("run.json"))
}

/// The file that holds what must stay away from the seats (outside the project when possible).
pub(crate) fn private_file(run: &Value, d: &Path) -> PathBuf {
    let base = crate::safe::private_dir();
    match crate::safe::make_dir(&base) {
        Ok(_) => {
            crate::safe::owner_only(&base);
            base.join(format!("{}.json", run.s("id")))
        }
        Err(_) => d.join(".private.json"),
    }
}

/// What must stay away from the seats: which seat wrote which blinded card.
pub(crate) fn private(run: &Value, d: &Path) -> Value {
    let f = private_file(run, d);
    crate::safe::read_own(&f)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

pub(crate) fn save_private(run: &Value, d: &Path, data: &Value) -> Result<()> {
    crate::safe::private_file(&private_file(run, d), &Json::ONE.dumps(data))
}

/// What a seat or the chair wrote in a JSON file, or `None`. Only a plain file of a sane size is
/// read (a link could point anywhere, a device never ends), and its text is cleaned of control
/// characters.
pub(crate) fn read_out(f: &Path) -> Option<Value> {
    crate::safe::plain_file(f, crate::safe::MAX_OUTPUT)?;
    providers::parse_json(&core::read_text(f).ok()?).map(crate::safe::cleaned)
}

/// A text file in the run's folders (a packet, a document the chair wrote): a plain file of a sane
/// size, without control characters.
pub(crate) fn read_doc(f: &Path) -> Result<String> {
    Ok(crate::safe::shown(&crate::safe::read_capped(f, crate::safe::MAX_OUTPUT)?).into_owned())
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
    crate::safe::make_dir(&p)?;
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
    if crate::safe::plain_file(&f, crate::safe::MAX_OUTPUT).is_some() {
        let body = core::read_text(&f).unwrap_or_default();
        let body = crate::safe::shown(&body);
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
    if let Ok(text) = crate::safe::read_capped(&f, crate::safe::MAX_OUTPUT) {
        for line in py::lines(&crate::safe::shown(&text)) {
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
