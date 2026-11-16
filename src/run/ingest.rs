//! `council ingest`: check what came back for the current step, record it and move the run on.
//!
//! A seat's output is either accepted whole or sent back with the reasons. A rejected file is moved
//! aside, so the same file is never rejected twice and a retry can land in its place.

use super::*;

// ----------------------------------------------------------------------------- reading the seats

/// The check for the outputs of the current step.
pub(crate) enum Check {
    /// Researchers and the mapper.
    Research(PathBuf),
    /// Experts, and the control arm beside the first ideation.
    Ideate,
    /// A blind ballot (shortlist vote or rank vote).
    Ballot {
        labels: Vec<String>,
        rids: Vec<String>,
    },
    /// The red team's findings.
    RedTeam { labels: Vec<String> },
    /// An elder's review.
    Elder {
        labels: Vec<String>,
        rids: Vec<String>,
    },
}

impl Check {
    fn of(&self, run: &Value, seat: &str, obj: &Value) -> Findings {
        match self {
            Check::Research(root) if seat == "MAP" => v_map(obj, root),
            Check::Research(root) => v_research(obj, root),
            Check::Ideate if seat == "CTRL" => v_ctrl(run, obj),
            Check::Ideate => v_ideate(run, seat, obj),
            Check::Ballot { labels, rids } => v_ballot(obj, labels, rids),
            Check::RedTeam { labels } => v_redteam(obj, labels),
            Check::Elder { labels, rids } => v_elder(obj, labels, rids),
        }
    }
}

/// The labels of a blinded packet, in packet order.
fn labels_in(mapping: &Value) -> Vec<String> {
    items(mapping).map(|(label, _)| label.clone()).collect()
}

/// The check for the outputs of the current step, or `None` when the chair (not a seat) acts.
pub(crate) fn validator(run: &Value, d: &Path) -> Result<Option<Check>> {
    Ok(match run.s("phase") {
        "research" => Some(Check::Research(core::project_root())),
        "ideate" => Some(Check::Ideate),
        ph @ ("vote" | "rank") => Some(Check::Ballot {
            labels: labels_in(&mapping_for(run, d, ph)?),
            rids: rubric_ids(run),
        }),
        "redteam" => Some(Check::RedTeam {
            labels: labels_in(&mapping_for(run, d, "redteam")?),
        }),
        "elders" => Some(Check::Elder {
            labels: labels_in(&mapping_for(run, d, "elders")?),
            rids: rubric_ids(run),
        }),
        _ => None,
    })
}

/// Why a seat still owes its output.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Owed {
    /// No output file yet.
    Missing,
    /// A file is there, but it is not valid.
    Invalid,
}

/// What the seats of a step have written so far.
pub(crate) struct Scanned {
    /// Valid outputs, by seat.
    pub good: IndexMap<String, Value>,
    /// Seats that still owe an output, and why.
    pub bad: IndexMap<String, (Owed, Vec<String>)>,
    /// Warnings on outputs that were kept.
    pub notes: Vec<String>,
}

/// Read what the seats of this step wrote. Pure: it changes nothing on disk or in the run.
pub(crate) fn scan_seats(run: &Value, d: &Path, check: &Check) -> Result<Scanned> {
    let mut sc = Scanned {
        good: IndexMap::new(),
        bad: IndexMap::new(),
        notes: Vec::new(),
    };
    for f in seat_files(run, d)? {
        let name = f
            .out
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !f.out.exists() && !crate::safe::is_link(&f.out) {
            sc.bad.insert(
                f.seat,
                (Owed::Missing, vec![format!("no output yet ({name})")]),
            );
            continue;
        }
        // a seat writes this file: it must be a plain one (a link could point at any file on the
        // machine, a device never ends) and of a sane size, or it is not read at all
        if crate::safe::plain_file(&f.out, crate::safe::MAX_OUTPUT).is_none() {
            let why = match std::fs::symlink_metadata(&f.out) {
                Ok(meta) if meta.file_type().is_file() => format!(
                    "the output is {} bytes; the limit is {}",
                    py::commas(meta.len() as i64),
                    py::commas(crate::safe::MAX_OUTPUT as i64)
                ),
                _ => "the output must be a plain file".to_string(),
            };
            sc.bad.insert(f.seat, (Owed::Invalid, vec![why]));
            continue;
        }
        let raw = core::read_text(&f.out)?;
        let document = name.ends_with(".md");
        let obj = if document {
            // a document: drop a code fence around the whole of it
            let text = crate::safe::unfence(&raw);
            let text = crate::safe::shown(&text);
            let text = py::strip(&text);
            (!text.is_empty()).then(|| json!({"markdown": text}))
        } else {
            providers::parse_json(&raw).map(crate::safe::cleaned)
        };
        let Some(obj) = obj else {
            let why = if document {
                "the output file is empty".to_string()
            } else {
                format!(
                    "the output is not a JSON object (it starts: {})",
                    py::str_repr(py::head(&py::squash(&crate::safe::shown(&raw)), 120))
                )
            };
            sc.bad.insert(f.seat, (Owed::Invalid, vec![why]));
            continue;
        };
        let (errs, warns) = check.of(run, &f.seat, &obj);
        sc.notes
            .extend(warns.iter().map(|w| format!("{}: {w}", f.seat)));
        if errs.is_empty() {
            sc.good.insert(f.seat, obj);
        } else {
            sc.bad.insert(f.seat, (Owed::Invalid, errs));
        }
    }
    Ok(sc)
}

/// Spawn lines for the seats that still owe an output. After a restart, finished seats are not redone.
pub(crate) fn seat_step(run: &Value, d: &Path) -> Result<Vec<String>> {
    let Some(check) = validator(run, d)? else {
        return Ok(vec!["THEN  council ingest".to_string()]);
    };
    let sc = scan_seats(run, d, &check)?;
    let total = sc.good.len() + sc.bad.len();
    if sc.bad.is_empty() {
        return Ok(vec![
            format!("All {total} seat(s) are in."),
            "THEN  council ingest".to_string(),
        ]);
    }
    let mut lines = Vec::new();
    if !sc.good.is_empty() {
        let mut names: Vec<&String> = sc.good.keys().collect();
        names.sort();
        let names: Vec<&str> = names.into_iter().map(String::as_str).collect();
        lines.push(format!(
            "{} of {total} seat(s) are already in ({}). Only these are still needed:",
            sc.good.len(),
            names.join(", ")
        ));
    }
    let missing: Vec<String> = sc
        .bad
        .iter()
        .filter(|(_, (kind, _))| *kind == Owed::Missing)
        .map(|(s, _)| s.clone())
        .collect();
    let invalid: Vec<&str> = sc
        .bad
        .keys()
        .filter(|s| !missing.contains(s))
        .map(String::as_str)
        .collect();
    if !invalid.is_empty() {
        lines.push(format!("{}: an output is there but not valid. `council ingest` says why and writes a retry prompt.", invalid.join(", ")));
    }
    if missing.is_empty() {
        lines.push("THEN  council ingest".to_string());
    } else {
        lines.extend(spawn_lines(run, d, Some(&missing))?);
    }
    Ok(lines)
}

/// How often a seat's output for this step was rejected.
fn attempts(run: &Value, seat: &str) -> i64 {
    run.at("attempts").int(&attempt_key(run, seat)).unwrap_or(0)
}

/// The key of a seat's attempts at the current step.
pub(crate) fn attempt_key(run: &Value, seat: &str) -> String {
    format!("{}/{}/{seat}", run.show("loop"), run.show("phase"))
}

/// `ceil(2n / 3)`: how many of n seats must be valid for a step to count.
fn two_thirds(n: usize) -> usize {
    (2 * n).div_ceil(3)
}

/// Accept the outputs of this step, or say exactly what is still owed. A rejected output is moved
/// aside (so the same file is never rejected twice and a retry can land) and counted once.
fn collect(
    run: &mut Value,
    d: &Path,
    check: &Check,
    quorum: usize,
    drop: &[String],
    reason: Option<&str>,
) -> Result<(IndexMap<String, Value>, Vec<String>)> {
    let Scanned {
        good,
        bad,
        mut notes,
    } = scan_seats(run, d, check)?;
    let files = seat_files(run, d)?;
    let file_of = |seat: &str| files.iter().find(|f| f.seat == seat);
    if !run["attempts"].is_object() {
        run["attempts"] = json!({});
    }
    let mut first_time: Vec<&String> = Vec::new();
    for (s, (kind, errs)) in &bad {
        let Some(f) = file_of(s).filter(|_| *kind == Owed::Invalid) else {
            continue;
        };
        let n = attempts(run, s) + 1;
        let key = attempt_key(run, s);
        run["attempts"][key] = json!(n);
        crate::log::record(
            d,
            json!({"event": "seat_output", "seat": s, "step": run.at("phase"), "state": format!("rejected (attempt {n})"),
                   "errors": errs.iter().take(8).collect::<Vec<_>>()}),
        );
        let name = f
            .out
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        crate::safe::rename(
            &f.out,
            &f.out
                .with_file_name(name.replace(".out.", &format!(".rejected-{n}."))),
        )?;
        if f.prompt.exists() {
            let listed: Vec<String> = errs.iter().map(|e| format!("- {e}")).collect();
            let retry = format!(
                "{}\n\n## Your previous output was rejected\n{}\nWrite the whole output again so that it is valid.\n",
                read_doc(&f.prompt)?,
                listed.join("\n")
            );
            core::write_text(&retry_file(&f.prompt), &retry)?;
        }
        if n == 1 {
            first_time.push(s);
        }
    }
    let drop: Vec<&str> = drop
        .iter()
        .map(|x| py::strip(x))
        .filter(|x| !x.is_empty())
        .collect();
    let mut problems: Vec<String> = Vec::new();
    if !drop.is_empty() && py::words(reason.unwrap_or("")) < 3 {
        problems.push("--drop needs --reason \"<why this seat is being left out>\"".to_string());
    }
    for s in &drop {
        if file_of(s).is_none() {
            let seats: Vec<&str> = files.iter().map(|f| f.seat.as_str()).collect();
            problems.push(format!(
                "{s} is not a seat of this step (seats: {})",
                seats.join(", ")
            ));
        } else if good.contains_key(*s) {
            problems.push(format!("{s} returned a valid output"));
        } else if first_time.iter().any(|x| x.as_str() == *s) {
            problems.push(format!(
                "{s} was rejected for the first time just now: retry it once first"
            ));
        }
    }
    let pending: Vec<String> = bad
        .keys()
        .filter(|s| !drop.contains(&s.as_str()))
        .cloned()
        .collect();
    let core_good = good.keys().filter(|s| *s != "CTRL").count();
    if !drop.is_empty() && problems.is_empty() && pending.is_empty() && core_good < quorum {
        problems.push(format!(
            "that would leave {core_good} valid seat(s); this step needs {quorum}"
        ));
    }
    if !pending.is_empty() || !problems.is_empty() {
        save(run, d)?;
        let mut lines = vec![format!(
            "Not all seats are in yet ({} of {} valid):",
            good.len(),
            files.len()
        )];
        for (s, (kind, errs)) in &bad {
            lines.push(match kind {
                Owed::Invalid => format!(
                    "  {s}: REJECTED (output {}): {}",
                    attempts(run, s),
                    errs.join("; ")
                ),
                Owed::Missing if attempts(run, s) > 0 => {
                    format!("  {s}: its retry has not come back yet")
                }
                Owed::Missing => format!("  {s}: no output yet"),
            });
        }
        if !pending.is_empty() {
            lines.extend(
                spawn_lines(run, d, Some(&pending))?
                    .iter()
                    .map(|line| format!("  {line}")),
            );
        }
        lines.extend(problems.iter().map(|x| format!("  cannot drop: {x}")));
        let hint = pending
            .iter()
            .find(|s| attempts(run, s) >= 2)
            .or(pending.first())
            .map(String::as_str)
            .unwrap_or("<seat>");
        lines.push(format!(
            "To go on without a seat that failed twice or never came back: council ingest --drop {hint} \
             --reason \"<why>\"   (this step needs {quorum} valid seat(s))"
        ));
        return fail(lines.join("\n"));
    }
    for s in &drop {
        let note = format!(
            "loop {} {}: seat {s} was left out after {} rejected output(s): {}",
            run.show("loop"),
            run.show("phase"),
            attempts(run, s),
            py::strip(reason.unwrap_or(""))
        );
        push(run, "deviations", json!(note));
    }
    if !run["hashes"].is_object() {
        run["hashes"] = json!({});
    }
    for (s, obj) in &good {
        if let Some(f) = file_of(s) {
            let rel = py::posix(f.out.strip_prefix(d).unwrap_or(&f.out));
            run["hashes"][rel] = json!(core::sha256_hex(&std::fs::read(&f.out)?));
        }
        let friction = text_of(obj.at("friction"));
        if !friction.is_empty() {
            let said = py::to_str(obj.at("friction"));
            let note = format!(
                "{s} ({}, loop {}): {}",
                run.show("phase"),
                run.show("loop"),
                py::head(py::strip(&said), 200)
            );
            push(run, "friction", json!(note));
        }
    }
    let phase = run.s("phase").to_string();
    let rejected: i64 = files.iter().map(|f| attempts(run, &f.seat)).sum();
    let so_far = run.at("calls").int(&phase).unwrap_or(0);
    run["calls"][phase] = json!(so_far + good.len() as i64 + rejected);
    notes.extend(harvest(run, d, &good));
    Ok((good, notes))
}

// ----------------------------------------------------------------------------- ingest

/// `council ingest`: accept the current step's outputs and print the next step.
pub fn cmd_ingest(run_id: Option<&str>, drop: Option<&str>, reason: Option<&str>) -> Result<i32> {
    let (mut run, d) = load(run_id)?;
    let phase = run.s("phase").to_string();
    if !PHASES.contains(&phase.as_str()) || matches!(phase.as_str(), "cut" | "done") {
        return fail(format!(
            "nothing to ingest in phase {}. Run `council next`.",
            py::repr(run.at("phase"))
        ));
    }
    let drop: Vec<String> = drop
        .unwrap_or("")
        .split(|c: char| c == ',' || py::is_space(c))
        .filter(|x| !x.is_empty())
        .map(str::to_string)
        .collect();
    if !drop.is_empty() && CHAIR_STEPS.contains(&phase.as_str()) {
        return fail(format!(
            "--drop names a seat to go on without; the {phase} step is yours, not a seat's"
        ));
    }
    let notes = match phase.as_str() {
        "direct" => ingest_direct(&mut run)?,
        "brief" => ingest_brief(&mut run, &d)?,
        "research" => ingest_research(&mut run, &d, &drop, reason)?,
        "ideate" => ingest_ideate(&mut run, &d, &drop, reason)?,
        "merge" => ingest_merge(&mut run, &d)?,
        "vote" => ingest_vote(&mut run, &d, &drop, reason)?,
        "redteam" => ingest_redteam(&mut run, &d, &drop, reason)?,
        "fuse" => ingest_fuse(&mut run, &d)?,
        "rank" => ingest_rank(&mut run, &d, &drop, reason)?,
        "elders" => ingest_elders(&mut run, &d, &drop, reason)?,
        "final" => ingest_final(&mut run, &d)?,
        _ => ingest_retro(&mut run, &d)?,
    };
    save(&mut run, &d)?;
    for n in notes {
        say!("{n}");
    }
    say!();
    let id = run.s("id").to_string();
    steps::cmd_next(Some(&id), None)
}

fn ingest_direct(run: &mut Value) -> Result<Vec<String>> {
    let res = core::verify_locks(&core::project_root(), None)?;
    run["locks"] = res.clone();
    if !res.truthy("ok") {
        return fail(format!(
            "locked files changed: {}. Fix the code, not the tests (or ask the requester to unlock).",
            py::repr(res.at("changed"))
        ));
    }
    run["phase"] = json!("done");
    Ok(vec![format!(
        "direct run closed{}",
        if res.s("note") == "no locks" {
            " (note: no files were locked)"
        } else {
            ""
        }
    )])
}

/// Values of `approved_by` that name no person. The chair fills in the brief; it may not approve it.
const NOT_A_PERSON: [&str; 21] = [
    "chair",
    "the chair",
    "claude",
    "self",
    "me",
    "myself",
    "ai",
    "assistant",
    "agent",
    "model",
    "auto",
    "none",
    "nobody",
    "n/a",
    "na",
    "yes",
    "true",
    "approved",
    "tbd",
    "todo",
    "?",
];

/// The sum of the rubric's weights, shown the way Python shows a sum of whole numbers and floats.
fn weight_sum(weights: &[&Value]) -> (f64, String) {
    let as_number = |w: &Value| match w {
        Value::Bool(b) => f64::from(u8::from(*b)),
        other => other.as_f64().unwrap_or(0.0),
    };
    let total: f64 = weights.iter().fold(0.0, |acc, w| acc + as_number(w));
    let whole = weights
        .iter()
        .all(|w| w.is_boolean() || as_int(w).is_some());
    (
        total,
        if whole {
            format!("{}", total as i64)
        } else {
            py::float_repr(total)
        },
    )
}

fn ingest_brief(run: &mut Value, d: &Path) -> Result<Vec<String>> {
    static ID_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^R\d+$").expect("valid"));
    let f = d.join("brief.yaml");
    if !f.exists() {
        return fail("brief.yaml is missing. Run `council next`.");
    }
    let loaded = match crate::yaml::load(&read_doc(&f)?) {
        Ok(value) => crate::safe::cleaned(value),
        Err(err) => return fail(format!("brief.yaml is not valid YAML: {err}")),
    };
    let mut b = if py::truthy(&loaded) {
        loaded
    } else {
        json!({})
    };
    if !b.is_object() {
        return fail("brief.yaml is not ready:\n  it must be a mapping (key: value lines), as in the template");
    }
    let mut errs: Vec<String> = Vec::new();
    if text_of(b.at("restated")).is_empty() {
        errs.push("restated is empty".to_string());
    }
    if !(1..=30).contains(&words_of(b.at("eli5"))) {
        errs.push("eli5 must be 1 to 25 words".to_string());
    }
    let mappings = |key: &str| -> Option<Vec<Value>> {
        match b.at(key) {
            Value::Array(list) if list.iter().all(Value::is_object) => Some(list.clone()),
            other if !py::truthy(other) => Some(Vec::new()),
            _ => None,
        }
    };
    match mappings("rubric") {
        None => errs.push(
            "rubric must be a list of criteria, each with an id, a name and a weight".to_string(),
        ),
        Some(rub) => {
            let bounds = run.at("shape").list("rubric");
            let (lo, hi) = (
                bounds.first().and_then(as_int).unwrap_or(0),
                bounds.get(1).and_then(as_int).unwrap_or(0),
            );
            if !(lo..=hi).contains(&(rub.len() as i64)) {
                errs.push(format!(
                    "rubric needs {lo} to {hi} criteria (has {})",
                    rub.len()
                ));
            }
            if rub
                .iter()
                .any(|r| text_of(r.at("name")).is_empty() || !ID_RE.is_match(&r.show("id")))
            {
                errs.push("every criterion needs an id like R1 and a name".to_string());
            }
            let ids: HashSet<String> = rub.iter().map(|r| r.show("id")).collect();
            if ids.len() != rub.len() {
                errs.push("rubric ids must be unique".to_string());
            }
            let weights: Vec<&Value> = rub.iter().map(|r| r.at("weight")).collect();
            if weights.iter().any(|w| !is_number(w)) {
                errs.push("every criterion needs a weight (a number)".to_string());
            } else {
                let (total, shown) = weight_sum(&weights);
                if total != 100.0 {
                    errs.push(format!(
                        "rubric weights must sum to 100 (they sum to {shown})"
                    ));
                }
            }
        }
    }
    let want = seats_of(run, "expert");
    match mappings("lenses") {
        None => errs.push("lenses must be a list of lenses, each with an id, a name, maximize, respect and method".to_string()),
        Some(lenses) => {
            let mut ids: Vec<String> = lenses.iter().map(|l| l.show("id")).collect();
            ids.sort();
            if ids != want {
                errs.push(format!("lenses must have exactly the ids {}", listed(&want)));
            }
            for l in &lenses {
                if ["name", "maximize", "respect", "method"].iter().any(|k| text_of(l.at(k)).is_empty()) {
                    errs.push(format!("lens {}: name, maximize, respect and method are all required", l.show("id")));
                }
            }
            let names: Vec<String> = lenses.iter().map(|l| text_of(l.at("name")).to_lowercase()).collect();
            if names.iter().collect::<HashSet<_>>().len() != names.len() {
                errs.push("two lenses share a name: make them orthogonal".to_string());
            }
        }
    }
    let who = b.s("approved_by").to_string();
    let who = py::strip(&who);
    if who.is_empty() || NOT_A_PERSON.contains(&who.to_lowercase().as_str()) {
        errs.push(
            "approved_by must name the person who approved this brief (or say \"requester\"), \
             or be \"unattended\". The chair cannot approve its own brief"
                .to_string(),
        );
    } else {
        b["approved_by"] = json!(who);
    }
    if !b.at("lessons_applied").is_array() {
        errs.push("lessons_applied must be a list (it may be empty)".to_string());
    }
    if words_of(b.at("facts")) > 500 {
        errs.push("facts is over 400 words: move the detail to facts.md".to_string());
    }
    let questions_ok = match b.at("research_questions") {
        Value::Null => true,
        Value::Array(list) => list.len() <= 8 && list.iter().all(Value::is_string),
        _ => false,
    };
    if !questions_ok {
        errs.push(
            "research_questions must be a list of at most 8 questions (it may be empty)"
                .to_string(),
        );
    }
    if !errs.is_empty() {
        return fail(format!("brief.yaml is not ready:\n  {}", errs.join("\n  ")));
    }
    let unattended = b.s("approved_by").to_lowercase() == "unattended";
    if unattended {
        b["approved_by"] = json!("unattended");
    }
    run["brief"] = b;
    if unattended && run.s("tier") == "full" {
        push(
            run,
            "deviations",
            json!("the brief was not approved by a person (unattended)"),
        );
    }
    run["loop"] = json!(1);
    push(run, "loops", json!({}));
    let (due, notes) = support_due(run, d);
    let research = !due.is_empty();
    cur_mut(run)["research_seats"] = json!(due);
    run["phase"] = json!(if research { "research" } else { "ideate" });
    let mut out = vec!["brief accepted: rubric frozen for this run".to_string()];
    out.extend(notes);
    Ok(out)
}

/// The items of a list field, tidied to a word cap; nothing when the field is not a list.
fn tidied(value: &Value, cap: usize, keep: usize) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .take(keep)
        .map(|item| memory::tidy(item, Some(cap)))
        .collect()
}

fn ingest_research(
    run: &mut Value,
    d: &Path,
    drop: &[String],
    reason: Option<&str>,
) -> Result<Vec<String>> {
    let root = core::project_root();
    let conf = memory_conf(run, &root);
    let before = run.at("shared").clone();
    let check = Check::Research(root.clone());
    let (good, mut notes) = collect(run, d, &check, 0, drop, reason)?;
    let seats = crate::val::strings(cur(run).at("research_seats"));
    let (came, left): (Vec<&String>, Vec<&String>) =
        seats.iter().partition(|x| good.contains_key(*x));
    let mut synthesis = Map::new();
    let mut lists: IndexMap<&str, Vec<String>> = [
        ("gaps", Vec::new()),
        ("conflicts", Vec::new()),
        ("queries", Vec::new()),
    ]
    .into_iter()
    .collect();
    for x in seats_of(run, "researcher") {
        let Some(report) = good.get(&x) else { continue };
        synthesis.insert(
            x.clone(),
            json!(memory::tidy(report.at("synthesis"), Some(70))),
        );
        for (key, cap, keep) in [("conflicts", 60, 6), ("queries", 16, 25), ("gaps", 40, 6)] {
            let held = &lists[key];
            let fresh: Vec<String> = tidied(report.at(key), cap, keep)
                .into_iter()
                .filter(|t| !t.is_empty() && !held.contains(t))
                .collect();
            lists[key].extend(fresh);
        }
    }
    let gain = |key: &str| run.at("shared").int(key).unwrap_or(0) - before.int(key).unwrap_or(0);
    let (added, merged) = (gain("facts_added"), gain("facts_merged"));
    let n_gaps = lists["gaps"].len();
    let rec = json!({"seats": came, "left_out": left, "synthesis": synthesis, "gaps": lists["gaps"], "conflicts": lists["conflicts"],
                     "queries": lists["queries"], "facts_added": added, "facts_merged": merged});
    if let Some(map) = good.get("MAP") {
        let run_id = run.s("id").to_string();
        let (path, note) = codemap::install_map(
            map.s("markdown"),
            &root,
            Some(&run_id),
            &codemap::scan(&root),
            "the mapper seat",
        )?;
        let rel = py::posix(path.strip_prefix(&root).unwrap_or(&path));
        let mut record = run.at("map").as_object().cloned().unwrap_or_default();
        record.insert("path".to_string(), json!(rel));
        record.insert("stale".to_string(), json!(false));
        record.insert("written_in".to_string(), json!(run_id));
        record.insert("reason".to_string(), json!("written in this run"));
        run["map"] = Value::Object(record);
        notes.push(format!(
            "codebase map written: {rel}{}",
            note.map(|n| format!(" ({n})")).unwrap_or_default()
        ));
        let copied = crate::safe::read_capped(&path, crate::safe::MAX_OUTPUT)
            .and_then(|text| memory::publish_map(&text, &rel, &conf))
            .and_then(|copy| memory::write_index(&conf, Some(&rel)).map(|_| copy));
        match copied {
            Ok(Some(copy)) => notes.push(format!("a copy for Obsidian: {}", copy.display())),
            Ok(None) => {}
            Err(err) => notes.push(format!(
                "the map could not be copied into the shared memory ({})",
                py::head(&err.to_string(), 120)
            )),
        }
    } else if seats.iter().any(|s| s == "MAP") {
        let mut record = run.at("map").as_object().cloned().unwrap_or_default();
        record.insert(
            "left_out".to_string(),
            json!(py::strip(reason.unwrap_or(""))),
        );
        run["map"] = Value::Object(record);
    }
    let no_research = !seats_of(run, "researcher").is_empty()
        && !came.iter().any(|x| *x != "MAP")
        && seats.iter().any(|x| x != "MAP");
    cur_mut(run)["research"] = rec;
    let upto = match cur(run).get("research_asked") {
        Some(n) => n.clone(),
        None => json!(run.list("commons").len()),
    };
    run["research_upto"] = upto;
    run["phase"] = json!("ideate");
    if no_research {
        notes.push(
            "NO RESEARCH came back: the council goes on with the facts in the brief only"
                .to_string(),
        );
    }
    notes.push(format!(
        "research in: {added} new fact(s) in the shared memory, {merged} already held{}",
        if n_gaps > 0 {
            format!("; not found: {n_gaps} gap(s) reported")
        } else {
            String::new()
        }
    ));
    Ok(notes)
}

/// The three fields of a card that is judged: `str(x or "").strip()` of each.
fn sealed_card(obj: &Value) -> Value {
    json!({"title": text_of(obj.at("title")), "body": text_of(obj.at("body")), "eli5": text_of(obj.at("eli5"))})
}

fn ingest_ideate(
    run: &mut Value,
    d: &Path,
    drop: &[String],
    reason: Option<&str>,
) -> Result<Vec<String>> {
    let n = seats_of(run, "expert").len();
    let with_control = wants_control(run);
    let (good, mut notes) = collect(run, d, &Check::Ideate, two_thirds(n), drop, reason)?;
    let (mut ideas, mut kept) = (Map::new(), Map::new());
    let (mut n_ideas, mut n_kept) = (0, 0);
    for (s, o) in good.iter().filter(|(s, _)| *s != "CTRL") {
        let own: Vec<&Value> = o
            .list("ideas")
            .iter()
            .filter(|x| !py::strip(x.as_str().unwrap_or("")).is_empty())
            .collect();
        let cards: Vec<Value> = o
            .list("cards")
            .iter()
            .map(|c| json!({"title": c.at("title"), "body": c.at("body"), "eli5": c.at("eli5"), "wins": c.at("wins"), "evidence": c.at("evidence")}))
            .collect();
        n_ideas += own.len();
        n_kept += cards.len();
        ideas.insert(s.clone(), json!(own));
        kept.insert(s.clone(), json!(cards));
    }
    let experts = ideas.len();
    let lp = cur_mut(run);
    lp["ideas"] = Value::Object(ideas);
    lp["kept"] = Value::Object(kept);
    if with_control {
        run["control"] = match good.get("CTRL") {
            // seal it: the card is read back from disk at the elder step
            Some(card) => json!({"done": true, "sha256": sha_value(&sealed_card(card))}),
            None => json!({"done": false, "dropped": py::strip(reason.unwrap_or(""))}),
        };
    }
    run["phase"] = json!("merge");
    notes.push(format!(
        "ideas in: {n_ideas} from {experts} experts; {n_kept} cards kept"
    ));
    Ok(notes)
}

/// A file the chair edits, as a JSON object. `None` when it is missing or holds no object.
fn chair_file(path: &Path) -> Result<Option<Value>> {
    Ok(read_out(path))
}

/// The fields of a card that the run keeps.
fn card_record(card: &Value, keys: [&str; 6]) -> Value {
    Value::Object(
        keys.iter()
            .map(|k| ((*k).to_string(), card.at(k).clone()))
            .collect(),
    )
}

fn ingest_merge(run: &mut Value, d: &Path) -> Result<Vec<String>> {
    let f = loop_dir(run, d)?.join("pool.json");
    let obj = chair_file(&f)?.filter(|o| py::truthy(o) && o.at("pool").is_array());
    let Some(obj) = obj else {
        return fail(format!(
            "{} must hold {{\"reviewed\": true, \"pool\": [...]}}",
            show(&f)
        ));
    };
    let mut pool: Vec<Value> = obj
        .list("pool")
        .iter()
        .filter(|c| c.is_object())
        .cloned()
        .collect();
    let mut errs = check_cards(&pool.iter().collect::<Vec<_>>(), "P");
    let kept: IndexMap<String, String> = kept_cards(run)
        .iter()
        .map(|c| (c.show("ref"), c.show("seat")))
        .collect();
    let mut used: HashSet<String> = HashSet::new();
    for c in &mut pool {
        // authors come from provenance, never from typing
        let refs: Option<Vec<&str>> = c
            .at("from")
            .as_array()
            .filter(|r| !r.is_empty())
            .and_then(|r| r.iter().map(Value::as_str).collect());
        let Some(refs) = refs.filter(|r| r.iter().all(|x| kept.contains_key(*x))) else {
            let example = kept.keys().next().cloned().unwrap_or_default();
            errs.push(format!(
                "{}: \"from\" must list the kept card(s) it came from, such as [\"{example}\"]",
                c.show("id")
            ));
            continue;
        };
        used.extend(refs.iter().map(|x| (*x).to_string()));
        let mut authors: Vec<&String> = refs.iter().filter_map(|x| kept.get(*x)).collect();
        authors.sort();
        authors.dedup();
        c["authors"] = json!(authors);
    }
    let lost: Vec<&str> = kept
        .keys()
        .filter(|x| !used.contains(*x))
        .map(String::as_str)
        .collect();
    if !lost.is_empty() && errs.is_empty() {
        errs.push(format!(
            "kept card(s) {} are in no pool card. Merge each into a card (add it to \"from\") or keep it as its own card: you merge, you do not judge",
            lost.join(", ")
        ));
    }
    if !obj.truthy("reviewed") {
        errs.push("set \"reviewed\": true once you have merged the duplicates".to_string());
    }
    if pool.len() < 2 {
        errs.push("the pool needs at least 2 cards".to_string());
    }
    if errs.is_empty() {
        errs.extend(crate::val::strings(
            core::blind(&pool, 0, core::caps::CARD, &seats_of(run, "expert"), SPREAD).at("errors"),
        ));
    }
    if !errs.is_empty() {
        return fail(format!("pool.json is not ready:\n  {}", errs.join("\n  ")));
    }
    let records: Vec<Value> = pool
        .iter()
        .map(|c| card_record(c, ["id", "title", "body", "eli5", "authors", "from"]))
        .collect();
    let ids: Vec<&Value> = pool.iter().map(|c| c.at("id")).collect();
    let n = n_fuse(run);
    let vote = run.s("tier") == "full" && pool.len() > n + 2;
    let lp = cur_mut(run);
    lp["pool"] = json!(records);
    if vote {
        run["phase"] = json!("vote");
        return Ok(vec![format!(
            "pool: {} cards -> shortlist vote",
            pool.len()
        )]);
    }
    lp["shortlist"] = json!(ids);
    run["phase"] = json!("redteam");
    Ok(vec![format!(
        "pool: {} cards (small enough: no shortlist vote, all go to the red team)",
        pool.len()
    )])
}

/// The ballots of a vote step: (which card is behind which label, the votes, notes for the chair).
fn ballots(
    run: &mut Value,
    d: &Path,
    phase: &str,
    drop: &[String],
    reason: Option<&str>,
) -> Result<(Value, Vec<Value>, Vec<String>)> {
    let mapping = mapping_for(run, d, phase)?;
    let (labels, rids, n) = (
        labels_in(&mapping),
        rubric_ids(run),
        seats_of(run, "expert").len(),
    );
    let check = Check::Ballot {
        labels,
        rids: rids.clone(),
    };
    let (good, notes) = collect(run, d, &check, two_thirds(n), drop, reason)?;
    let votes = good
        .iter()
        .map(|(s, o)| {
            let best: Map<String, Value> = items(o.at("best_on"))
                .filter(|(k, _)| rids.contains(k))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            json!({"voter": s, "ranking": o.at("ranking"), "best_on": best})
        })
        .collect();
    Ok((mapping, votes, notes))
}

/// Label pairs as card-id pairs.
fn id_pairs(pairs: &Value, mapping: &Value) -> Vec<Value> {
    pairs
        .as_array()
        .into_iter()
        .flatten()
        .map(|pair| {
            json!(pair
                .as_array()
                .into_iter()
                .flatten()
                .map(|l| mapping.at(&py::to_str(l)).at("id"))
                .collect::<Vec<_>>())
        })
        .collect()
}

/// The facts of a vote that the record keeps besides its ranking.
fn vote_facts(record: &mut Value, res: &Value, mapping: &Value) {
    for key in ["ballots", "head_to_heads", "recused", "gaps"] {
        record[key] = res.at(key).clone();
    }
    record["thin"] = json!(id_pairs(res.at("thin"), mapping));
}

fn ingest_vote(
    run: &mut Value,
    d: &Path,
    drop: &[String],
    reason: Option<&str>,
) -> Result<Vec<String>> {
    let (mapping, votes, mut notes) = ballots(run, d, "vote", drop, reason)?;
    let res = core::tally(&votes, &mapping, &core::TallyOptions::default());
    let order: Vec<String> = res
        .list("ranking")
        .iter()
        .map(|r| r.show("label"))
        .collect();
    let n = (n_fuse(run) + 2).min(order.len());
    let spared = core::protected(&order, n, res.at("thin"), &HashSet::new());
    let ranking: Vec<Value> = res
        .list("ranking")
        .iter()
        .map(|r| json!({"id": r.at("id"), "score": r.at("score")}))
        .collect();
    let mut record = json!({"ranking": ranking, "agreement": res.at("agreement"), "agreement_note": res.at("agreement_note")});
    vote_facts(&mut record, &res, &mapping);
    let id_of = |l: &String| mapping.at(l).at("id").clone();
    let shortlist: Vec<Value> = order[..n].iter().chain(spared.iter()).map(id_of).collect();
    let lp = cur_mut(run);
    lp["vote"] = record;
    lp["shortlist"] = json!(shortlist);
    if !spared.is_empty() {
        let ids: Vec<String> = spared.iter().map(|l| mapping.at(l).show("id")).collect();
        let note = format!(
            "loop {} vote: {} stayed on the shortlist although ranked below the line: the vote could not judge it",
            run.show("loop"),
            ids.join(", ")
        );
        push(run, "deviations", json!(note));
    }
    run["phase"] = json!("redteam");
    notes.push(format!(
        "shortlist vote: {} ballots, agreement {} ({}); {} cards go to the red team",
        res.show("ballots"),
        res.show("agreement"),
        res.show("agreement_note"),
        shortlist.len()
    ));
    Ok(notes)
}

fn ingest_redteam(
    run: &mut Value,
    d: &Path,
    drop: &[String],
    reason: Option<&str>,
) -> Result<Vec<String>> {
    let mapping = mapping_for(run, d, "redteam")?;
    let labels = labels_in(&mapping);
    let check = Check::RedTeam {
        labels: labels.clone(),
    };
    let (good, mut notes) = collect(run, d, &check, 0, drop, reason)?;
    let report = good.get("RT");
    let mut findings: Vec<Value> = Vec::new();
    for f in report
        .map(|r| r.list("findings"))
        .into_iter()
        .flatten()
        .take(10)
    {
        let cid = match target_of(f.at("candidate"), &labels).as_deref() {
            Some("all") | None => json!("all"),
            Some(label) => mapping.at(label).at("id").clone(),
        };
        findings.push(json!({"n": findings.len() + 1, "candidate_id": cid, "severity": f.at("severity"),
                             "finding": py::strip(&f.show("finding")), "evidence": text_of(f.at("evidence")), "fix": text_of(f.at("fix"))}));
    }
    let count = |severity: &str| {
        findings
            .iter()
            .filter(|f| f.s("severity") == severity)
            .count()
    };
    let line = format!(
        "red team: {} findings (S1 {}, S2 {}, S3 {})",
        findings.len(),
        count("S1"),
        count("S2"),
        count("S3")
    );
    let lp = cur_mut(run);
    lp["red_team"] = json!(findings);
    if report.is_none() {
        lp["red_team_skipped"] = json!(py::strip(reason.unwrap_or("")));
    }
    if let Some(eli5) = report.map(|r| r.at("eli5")).filter(|e| py::truthy(e)) {
        lp["red_team_eli5"] = eli5.clone();
    }
    run["phase"] = json!("fuse");
    notes.push(line);
    Ok(notes)
}

/// A finding's number as the chair typed it (Python counts `true` as 1).
fn finding_no(value: &Value) -> Option<f64> {
    match value {
        Value::Bool(b) => Some(f64::from(u8::from(*b))),
        other => other.as_f64(),
    }
}

fn ingest_fuse(run: &mut Value, d: &Path) -> Result<Vec<String>> {
    let f = loop_dir(run, d)?.join("candidates.json");
    let obj = chair_file(&f)?.filter(|o| py::truthy(o) && o.at("candidates").is_array());
    let Some(obj) = obj else {
        return fail(format!(
            "{} must hold {{\"candidates\": [...], \"responses\": [...], \"checks\": {{}}}}",
            show(&f)
        ));
    };
    let lp = cur(run).clone();
    let mut cands: Vec<Value> = obj
        .list("candidates")
        .iter()
        .filter(|c| c.is_object())
        .cloned()
        .collect();
    let mut errs = check_cards(&cands.iter().collect::<Vec<_>>(), "C");
    let pool: IndexMap<String, Value> = shortlisted(&lp)
        .into_iter()
        .map(|c| (c.show("id"), c))
        .collect();
    for c in &mut cands {
        // authors come from provenance, never from typing
        let parents: Option<Vec<&str>> = c
            .at("parents")
            .as_array()
            .filter(|p| !p.is_empty())
            .and_then(|p| p.iter().map(Value::as_str).collect());
        let Some(parents) = parents.filter(|p| p.iter().all(|x| pool.contains_key(*x))) else {
            let ids: Vec<&str> = pool.keys().map(String::as_str).collect();
            errs.push(format!(
                "{}: \"parents\" must list the shortlisted pool card(s) it was built from ({})",
                c.show("id"),
                ids.join(", ")
            ));
            continue;
        };
        let mut authors: Vec<String> = parents
            .iter()
            .flat_map(|x| crate::val::strings(pool[*x].at("authors")))
            .collect();
        authors.sort();
        authors.dedup();
        c["authors"] = json!(authors);
    }
    let want = n_fuse(run).min(lp.list("shortlist").len());
    if cands.len() != want {
        errs.push(format!(
            "this loop needs exactly {want} candidates (has {})",
            cands.len()
        ));
    }
    let responses: Vec<&Value> = obj
        .list("responses")
        .iter()
        .filter(|x| x.is_object() && finding_no(x.at("finding")).is_some())
        .collect();
    let response_to = |n: &Value| {
        responses
            .iter()
            .rev()
            .find(|x| finding_no(x.at("finding")) == finding_no(n))
            .copied()
    };
    for x in lp
        .list("red_team")
        .iter()
        .filter(|x| matches!(x.s("severity"), "S1" | "S2"))
    {
        let action = response_to(x.at("n")).map(|a| a.s("action")).unwrap_or("");
        if !matches!(action, "fixed" | "rebutted" | "accepted") {
            errs.push(format!(
                "finding {} ({}) needs action fixed | rebutted | accepted",
                x.show("n"),
                x.show("severity")
            ));
        } else if action != "fixed"
            && response_to(x.at("n"))
                .map(|a| words_of(a.at("note")))
                .unwrap_or(0)
                < 5
        {
            let article = if action == "accepted" { "an" } else { "a" };
            errs.push(format!("finding {}: {article} {action} finding needs a note (evidence, or why the risk is acceptable)", x.show("n")));
        }
    }
    let ids: Vec<String> = cands.iter().map(|c| c.show("id")).collect();
    let checks = obj.at("checks");
    let results: Vec<(&String, String)> = items(checks)
        .map(|(k, v)| (k, py::to_str(v).to_lowercase()))
        .collect();
    let mapped = checks.is_object() || !py::truthy(checks);
    if !mapped
        || results
            .iter()
            .any(|(k, v)| !ids.contains(k) || !matches!(v.as_str(), "pass" | "fail" | "n/a"))
    {
        errs.push("checks must map candidate ids to pass | fail | n/a".to_string());
    }
    if errs.is_empty() {
        errs.extend(crate::val::strings(
            core::blind(
                &cands,
                0,
                core::caps::CARD,
                &seats_of(run, "expert"),
                SPREAD,
            )
            .at("errors"),
        ));
    }
    if !errs.is_empty() {
        return fail(format!(
            "candidates.json is not ready:\n  {}",
            errs.join("\n  ")
        ));
    }
    if want < n_fuse(run) {
        let note = format!(
            "loop {}: only {want} candidates (the pool was small)",
            run.show("loop")
        );
        push(run, "deviations", json!(note));
    }
    let mut red_team = lp.list("red_team").clone();
    for x in &mut red_team {
        if let Some(a) = response_to(x.at("n")) {
            x["response"] = json!({"action": a.at("action"), "note": text_of(a.at("note"))});
        }
    }
    let records: Vec<Value> = cands
        .iter()
        .map(|c| card_record(c, ["id", "title", "body", "eli5", "authors", "parents"]))
        .collect();
    let not_carried: Vec<&String> = pool
        .keys()
        .filter(|p| {
            !cands
                .iter()
                .any(|c| crate::val::strings(c.at("parents")).contains(p))
        })
        .collect();
    let note = if not_carried.is_empty() {
        String::new()
    } else {
        format!(
            "; not carried into any candidate: {}",
            not_carried
                .iter()
                .map(|p| p.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let checks: Map<String, Value> = results
        .into_iter()
        .map(|(k, v)| (k.clone(), json!(v)))
        .collect();
    let fused = cands.len();
    let rec = cur_mut(run);
    rec["red_team"] = json!(red_team);
    rec["candidates"] = json!(records);
    rec["not_carried"] = json!(not_carried);
    rec["checks"] = Value::Object(checks);
    run["phase"] = json!("rank");
    Ok(vec![format!(
        "{fused} candidates fused; every S1/S2 finding has a response{note}"
    )])
}

fn ingest_rank(
    run: &mut Value,
    d: &Path,
    drop: &[String],
    reason: Option<&str>,
) -> Result<Vec<String>> {
    let (mapping, votes, mut notes) = ballots(run, d, "rank", drop, reason)?;
    let label_of: HashMap<String, &String> =
        items(&mapping).map(|(l, v)| (v.show("id"), l)).collect();
    let checks: Map<String, Value> = items(cur(run).at("checks"))
        .filter_map(|(c, res)| label_of.get(c).map(|l| ((*l).clone(), res.clone())))
        .collect();
    let checks = Value::Object(checks);
    let res = core::tally(
        &votes,
        &mapping,
        &core::TallyOptions {
            checks: Some(&checks),
            ..Default::default()
        },
    );
    let order: Vec<Value> = res
        .list("ranking")
        .iter()
        .map(|x| x.at("id").clone())
        .collect();
    let ranking: Vec<Value> = res
        .list("ranking")
        .iter()
        .map(|x| json!({"id": x.at("id"), "score": x.at("score"), "check": x.at("check")}))
        .collect();
    let mut record = json!({"order": order, "ranking": ranking, "agreement": res.at("agreement"), "agreement_note": res.at("agreement_note")});
    vote_facts(&mut record, &res, &mapping);
    record["ties"] = json!(id_pairs(res.at("ties"), &mapping));
    let best: Map<String, Value> = items(res.at("best_on"))
        .map(|(crit, l)| (crit.clone(), mapping.at(&py::to_str(l)).at("id").clone()))
        .collect();
    record["best_on"] = Value::Object(best);
    let shown: Vec<String> = order.iter().map(py::to_str).collect();
    cur_mut(run)["rank"] = record;
    run["phase"] = json!("elders");
    notes.push(format!(
        "rank vote: {} ballots, agreement {} ({}); order {}",
        res.show("ballots"),
        res.show("agreement"),
        res.show("agreement_note"),
        shown.join(" > ")
    ));
    Ok(notes)
}

fn ingest_elders(
    run: &mut Value,
    d: &Path,
    drop: &[String],
    reason: Option<&str>,
) -> Result<Vec<String>> {
    let mapping = mapping_for(run, d, "elders")?;
    let (labels, rids) = (labels_in(&mapping), rubric_ids(run));
    let check = Check::Elder {
        labels: labels.clone(),
        rids,
    };
    let (good, mut notes) = collect(run, d, &check, 1, drop, reason)?;
    let ident = |l: &str| mapping.at(l).at("id").clone();
    let mut seats: Vec<&String> = good.keys().collect();
    seats.sort();
    let mut reviews: Vec<Value> = Vec::new();
    for s in seats {
        let (o, v) = (&good[s], run.at("seats").at(s));
        let (scores, verdict) = (
            by_label(o.at("scores"), &labels),
            by_label(o.at("verdict"), &labels),
        );
        let preferred = core::clean_label(o.at("preferred"), &labels)
            .map(|l| ident(&l))
            .unwrap_or(Value::Null);
        let verdicts: Map<String, Value> = labels
            .iter()
            .map(|l| {
                (
                    py::to_str(&ident(l)),
                    json!(
                        py::strip(&verdict.get(l).map(py::to_str).unwrap_or_default())
                            .to_lowercase()
                    ),
                )
            })
            .collect();
        let scored: Map<String, Value> = labels
            .iter()
            .map(|l| {
                (
                    py::to_str(&ident(l)),
                    scores
                        .get(l)
                        .filter(|x| x.is_object())
                        .cloned()
                        .unwrap_or_else(|| json!({})),
                )
            })
            .collect();
        let risks: Vec<&Value> = o
            .list("top_risks")
            .iter()
            .take(3)
            .filter(|x| x.is_object())
            .collect();
        reviews.push(json!({"seat": s, "model": v.at("model"), "stand_in_for": v.at("stand_in_for"), "model_source": v.at("model_source"),
                            "preferred": preferred, "verdict": verdicts, "scores": scored, "confidence": o.at("confidence"),
                            "top_risks": risks, "key_fix": text_of(o.at("key_fix")), "own_idea": text_of(o.at("own_idea")),
                            "eli5": text_of(o.at("eli5"))}));
    }
    let n = reviews.len();
    cur_mut(run)["elder_reviews"] = json!(reviews);
    run["phase"] = json!("cut");
    notes.push(format!("{n} elder review(s) in"));
    Ok(notes)
}

/// A file the chair wrote, or nothing when it is not there.
fn written(path: &Path) -> Result<String> {
    Ok(if path.exists() {
        read_doc(path)?
    } else {
        String::new()
    })
}

fn ingest_final(run: &mut Value, d: &Path) -> Result<Vec<String>> {
    let text = written(&d.join("final.md"))?;
    let mut errs: Vec<String> = Vec::new();
    let todo = text.matches("TODO(chair)").count();
    if todo > 0 {
        errs.push(format!("{todo} TODO(chair) marker(s) left"));
    }
    for needle in [
        "**Answer:**",
        "**ELI5:**",
        "## Risks",
        "## Dissent",
        "## How we'll know",
    ] {
        if !text.contains(needle) {
            errs.push(format!("missing section: {needle}"));
        }
    }
    let lower = text.to_lowercase();
    let stand_ins = items(run.at("seats")).any(|(_, v)| v.truthy("stand_in_for"));
    if stand_ins && !lower.contains("stand-in") {
        errs.push("this run used stand-in seats: the write-up must say so".to_string());
    }
    if run.s("control_result") == "lost"
        && !lower.contains("control")
        && !lower.contains("baseline")
    {
        errs.push("the control arm won: the write-up must say so".to_string());
    }
    if !errs.is_empty() {
        return fail(format!("final.md is not ready:\n  {}", errs.join("\n  ")));
    }
    run["final"] = json!({"file": "final.md", "words": py::words(&text)});
    run["phase"] = json!("retro");
    Ok(vec!["final accepted".to_string()])
}

fn ingest_retro(run: &mut Value, d: &Path) -> Result<Vec<String>> {
    let text = written(&d.join("retro.md"))?;
    let todo = text.matches("TODO(chair)").count();
    if todo > 0 {
        return fail(format!("retro.md has {todo} TODO(chair) marker(s) left"));
    }
    let body = text
        .split_once("## Lessons")
        .map(|(_, rest)| rest)
        .unwrap_or(&text);
    let body = body
        .split_once("## Lessons used")
        .map(|(own, _)| own)
        .unwrap_or(body);
    let lines: Vec<&str> = py::lines(body)
        .into_iter()
        .map(py::strip)
        .filter(|l| l.starts_with("- ["))
        .collect();
    let bad: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|l| !LESSON_RE.is_match(l))
        .collect();
    if !bad.is_empty() {
        return fail(format!(
            "lesson lines must look like `- [L-YYYYMMDD-1] (helpful: 0 | harmful: 0) Text. Evidence: ...`:\n  {}",
            bad.join("\n  ")
        ));
    }
    if lines.len() > 3 {
        return fail(format!(
            "{} lessons: keep the 3 that a future run can act on",
            lines.len()
        ));
    }
    let proposed = d.join("lessons.proposed.md");
    core::write_text(
        &proposed,
        &format!(
            "{}{}",
            lines.join("\n"),
            if lines.is_empty() { "" } else { "\n" }
        ),
    )?;
    run["retro"] = json!({"file": "retro.md", "lessons": lines});
    run["phase"] = json!("done");
    let unblinded = d.join("unblinded.json");
    core::write_text(&unblinded, &Json::ONE.dumps(&private(run, d)))?; // who wrote what, now that it is over
    if run.truthy("memory") {
        // the shared memory gets its names at the same moment
        let conf = run.at("memory").clone();
        let no_authors = json!({});
        let authors = if run.truthy("unsigned_facts") {
            run.at("unsigned_facts")
        } else {
            &no_authors
        };
        let signed = memory::sign(authors, run.at("seats"), &conf)
            .and_then(|_| memory::write_commons(run, &conf, true))
            .and_then(|_| memory::write_index(&conf, run.at("map").at("path").as_str()));
        if let Err(err) = signed {
            let note = format!(
                "the shared memory could not be signed at the end ({})",
                py::head(&err.to_string(), 120)
            );
            push(run, "deviations", json!(note));
        }
    }
    let check = core::check_run(run);
    let verdict = if check.truthy("ok") {
        "ok".to_string()
    } else {
        format!("ERRORS {}", py::repr(check.at("errors")))
    };
    let warnings = if check.truthy("warnings") {
        py::repr(check.at("warnings"))
    } else {
        "none".to_string()
    };
    Ok(vec![
        format!(
            "retro accepted: {} lesson(s) proposed in {}",
            lines.len(),
            show(&proposed)
        ),
        format!(
            "the run is unblinded for the record: {} (check it any time with `council audit`)",
            show(&unblinded)
        ),
        format!("protocol check: {verdict}; warnings: {warnings}"),
    ])
}
