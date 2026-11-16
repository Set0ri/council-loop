//! `council status`, `note`, `deviation` and `audit`: read a run, add to its record, and re-count it.

use super::*;

/// The chair's own post to the commons: a thought for every member, or a question for the researchers.
pub fn cmd_note(run_id: Option<&str>, text: &str, to: &str) -> Result<i32> {
    let (mut run, d) = load(run_id)?;
    if !run.truthy("loop") || !run.has("memory") {
        return fail("notes belong to a council run that has a brief: ingest the brief first");
    }
    let note = match memory::clean_note(&json!({"to": to, "text": text})) {
        Ok(note) => note,
        Err(problem) => return fail(format!("not posted: {problem}")),
    };
    let post = json!({"loop": run.at("loop"), "phase": run.at("phase"), "by": "chair", "role": "chair", "to": note.at("to"), "text": note.at("text")});
    push(&mut run, "commons", post);
    if !run.has("shared") {
        run["shared"] =
            json!({"facts_added": 0, "facts_merged": 0, "facts_refused": 0, "notes": 0});
    }
    let notes = run.at("shared").int("notes").unwrap_or(0) + 1;
    run["shared"]["notes"] = json!(notes);
    let conf = run.at("memory").clone();
    let page = memory::write_commons(&run, &conf, false)?;
    save(&mut run, &d)?;
    let has_researchers = !seats_of(&run, "researcher").is_empty();
    let asked = if run.s("phase") == "research" && has_researchers {
        "this round's researchers get it: run `council next` again to rewrite their packets"
    } else if has_researchers
        && !is_last(&run)
        && sup(&run).at("research").s("later_loops") != "never"
    {
        "the researchers take it up before the next loop"
    } else {
        "no research round is left in this run: it is listed as open in the retro"
    };
    let when = match note.s("to") {
        "chair" => "it is kept for the record",
        "research" => asked,
        _ => "members read it, unsigned, in the next ideation or red-team packet",
    };
    say!("posted to {}: {when}", note.s("to"));
    say!(
        "commons: {}",
        page.map(|p| show(&p)).unwrap_or_else(|| "None".to_string())
    );
    Ok(0)
}

/// The chair's own log of anything done outside the protocol. It reaches the ledger, the final and the retro.
pub fn cmd_deviation(run_id: Option<&str>, text: &str) -> Result<i32> {
    let (mut run, d) = load(run_id)?;
    let text = py::squash(text);
    if py::words(&text) < 4 {
        return fail("say what was done differently and why (one line)");
    }
    let place = if run.truthy("loop") {
        format!("loop {} {}", run.show("loop"), run.show("phase"))
    } else {
        run.show("phase")
    };
    push(&mut run, "deviations", json!(format!("{place}: {text}")));
    save(&mut run, &d)?;
    say!(
        "recorded deviation {}: {place}: {text}",
        run.list("deviations").len()
    );
    Ok(0)
}

/// `council status`: where the run stands, for a chair that lost its context.
pub fn cmd_status(run_id: Option<&str>) -> Result<i32> {
    let (run, d) = load(run_id)?;
    say!("{}", run.show("gate"));
    let phase = run.show("phase");
    say!(
        "run {} · phase {phase}{}",
        run.s("id"),
        if run.truthy("loop") {
            format!(" · loop {}/{}", run.show("loop"), run.show("max_loops"))
        } else {
            String::new()
        }
    );
    say!("ask: {}", py::head(&run.show("ask"), 300));
    let b = run.at("brief");
    if py::truthy(b) {
        say!("brief ELI5: {}", b.show("eli5"));
        let rubric: Vec<String> = b
            .list("rubric")
            .iter()
            .map(|x| format!("{} {} ({})", x.show("id"), x.show("name"), x.show("weight")))
            .collect();
        say!("rubric: {}", rubric.join(", "));
        let lenses: Vec<String> = b
            .list("lenses")
            .iter()
            .map(|l| format!("{} {}", l.show("id"), l.show("name")))
            .collect();
        say!("lenses: {}", lenses.join(", "));
    }
    for (k, lp) in run.list("loops").iter().enumerate() {
        if lp.has("survivors") {
            let titles = titles_of(lp);
            let kept: Vec<String> = crate::val::strings(lp.at("survivors"))
                .iter()
                .map(|c| format!("{c} {}", titles.get(c).cloned().unwrap_or_default()))
                .collect();
            say!(
                "loop {}: {}  -> kept {}",
                k + 1,
                lp.show("eli5"),
                kept.join("; ")
            );
        } else if py::truthy(lp) {
            let got: Vec<&str> = [
                "research",
                "ideas",
                "pool",
                "red_team",
                "candidates",
                "rank",
                "elder_reviews",
            ]
            .into_iter()
            .filter(|x| lp.has(x))
            .collect();
            say!(
                "loop {}: in progress{}",
                k + 1,
                if got.is_empty() {
                    String::new()
                } else {
                    format!(" ({} done)", got.join(", "))
                }
            );
        }
    }
    if run.truthy("winner") {
        say!(
            "winner: {} · control arm: {}",
            run.show("winner"),
            run.show("control_line")
        );
    }
    let deviations = crate::val::strings(run.at("deviations"));
    if !deviations.is_empty() {
        let short: Vec<&str> = deviations.iter().map(|x| py::head(x, 140)).collect();
        say!("deviations ({}): {}", deviations.len(), short.join("; "));
    }
    if run.truthy("memory") {
        let (sh, m, mem) = (run.at("shared"), run.at("map"), run.at("memory"));
        let count = |key: &str| {
            sh.get(key)
                .map(py::to_str)
                .unwrap_or_else(|| "0".to_string())
        };
        say!(
            "shared memory: {} at {} · this run: {} fact(s) added, {} note(s), {} question(s) waiting for the researchers",
            mem.show("backend"),
            mem.show("dir"),
            count("facts_added"),
            count("notes"),
            open_requests(&run).len()
        );
        if py::truthy(m) {
            say!("codebase map: {} ({})", m.show("path"), m.show("reason"));
        }
    }
    say!(
        "calls so far: {} (estimate {})",
        calls_so_far(&run),
        run.show("calls_estimate")
    );
    // a missing mapping means the step's packets are not written yet
    let check = match validator(&run, &d) {
        Ok(check) => check,
        Err(err) if err.downcast_ref::<core::UserError>().is_some() => None,
        Err(err) => return Err(err),
    };
    if let Some(check) = check {
        let sc = scan_seats(&run, &d, &check)?;
        if sc.bad.is_empty() {
            say!(
                "this step: all {} seat(s) in. Run `council ingest`.",
                sc.good.len()
            );
        } else {
            let owed: Vec<String> = sc
                .bad
                .iter()
                .map(|(s, (kind, _))| {
                    let what = match kind {
                        Owed::Invalid => "output not checked yet",
                        Owed::Missing if run.at("attempts").truthy(&attempt_key(&run, s)) => {
                            "retry owed"
                        }
                        Owed::Missing => "missing",
                    };
                    format!("{s} ({what})")
                })
                .collect();
            say!(
                "this step: {} of {} seat(s) in; still owed: {}",
                sc.good.len(),
                sc.good.len() + sc.bad.len(),
                owed.join(", ")
            );
        }
    } else if CHAIR_STEPS.contains(&phase.as_str()) || phase == "cut" {
        say!("this step is yours ({phase}).");
    }
    if phase == "done" {
        say!("next: nothing, the run is finished. The decision: final.md in the run folder · re-count the votes: council audit");
    } else {
        say!("next: council next        # prints the exact commands for this step; safe to run again");
    }
    Ok(0)
}

/// The ballots of a vote step as they are on disk, sorted by file name.
fn ballots_on_disk(dir: &Path) -> Result<Vec<Value>> {
    let mut files: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(entries) => entries.filter_map(|e| e.ok()).map(|e| e.path()).collect(),
        Err(_) => Vec::new(),
    };
    files.retain(|f| {
        f.file_name()
            .is_some_and(|n| n.to_string_lossy().ends_with(".out.json"))
    });
    files.sort();
    let mut votes = Vec::new();
    for f in files {
        let Some(obj) = read_out(&f) else {
            continue;
        };
        let name = f
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let or = |key: &str, empty: Value| {
            if obj.truthy(key) {
                obj.at(key).clone()
            } else {
                empty
            }
        };
        votes.push(json!({"voter": name.split('.').next().unwrap_or(""), "ranking": or("ranking", json!([])), "best_on": or("best_on", json!({}))}));
    }
    Ok(votes)
}

/// Re-count every vote from the ballots on disk and compare with the record. Also checks that the
/// accepted seat outputs and the sealed control card are the files that were ingested.
pub fn cmd_audit(run_id: Option<&str>) -> Result<i32> {
    let (run, d) = load(run_id)?;
    let mut maps = private(&run, &d);
    let arch = d.join("unblinded.json");
    if arch.exists() {
        let archived: Value = serde_json::from_str(&crate::safe::read_own(&arch)?)
            .map_err(|e| anyhow::anyhow!("{} is not valid JSON: {e}", arch.display()))?;
        maps = Value::Object(crate::val::merged(&archived, &maps));
    }
    let mut bad = 0usize;
    for (k, lp) in run.list("loops").iter().enumerate() {
        let k = k + 1;
        for phase in ["vote", "rank"] {
            let rec = lp.at(phase);
            if !py::truthy(rec) {
                continue;
            }
            let mapping = maps.at(&format!("{k}/{phase}"));
            if !py::truthy(mapping) {
                bad += 1;
                say!("loop {k} {phase}: CANNOT RE-COUNT (the who-wrote-what mapping is not available here)");
                continue;
            }
            let votes = ballots_on_disk(&d.join(format!("loop-{k}")).join(phase))?;
            let label_of: HashMap<String, &String> =
                items(mapping).map(|(l, v)| (v.show("id"), l)).collect();
            let checks: Map<String, Value> = items(lp.at("checks"))
                .filter_map(|(c, x)| label_of.get(c).map(|l| ((*l).clone(), x.clone())))
                .collect();
            let checks = Value::Object(checks);
            let opt = core::TallyOptions {
                checks: (phase == "rank").then_some(&checks),
                ..Default::default()
            };
            let res = core::tally(&votes, mapping, &opt);
            let again: Vec<String> = res.list("ranking").iter().map(|x| x.show("id")).collect();
            let then: Vec<String> = rec.list("ranking").iter().map(|x| x.show("id")).collect();
            let ok = again == then && rec.int("ballots") == Some(votes.len() as i64);
            bad += usize::from(!ok);
            say!(
                "loop {k} {phase}: {}  recorded {} ({} ballots){}",
                if ok { "ok" } else { "MISMATCH" },
                then.join(" > "),
                rec.show("ballots"),
                if ok {
                    String::new()
                } else {
                    format!("; re-count {} ({} ballots)", again.join(" > "), votes.len())
                }
            );
        }
    }
    let hashes: Vec<(&String, &Value)> = items(run.at("hashes")).collect();
    let changed: Vec<&str> = hashes
        .iter()
        .filter(|(rel, h)| {
            let file = d.join(rel.as_str());
            !file.exists()
                || std::fs::read(&file)
                    .map(|bytes| core::sha256_hex(&bytes))
                    .ok()
                    .as_deref()
                    != h.as_str()
        })
        .map(|(rel, _)| rel.as_str())
        .collect();
    bad += changed.len();
    say!(
        "seat outputs: {} of {} match what was ingested{}{}",
        hashes.len() - changed.len(),
        hashes.len(),
        if changed.is_empty() {
            String::new()
        } else {
            format!("; CHANGED OR MISSING: {}", changed.join(", "))
        },
        if hashes.is_empty() {
            " (this run was recorded before outputs were hashed)"
        } else {
            ""
        }
    );
    if run.at("control").truthy("done") {
        match control_card(&run, &d) {
            Ok(_) if run.at("control").truthy("sha256") => say!("control arm: sealed card intact"),
            Ok(_) => say!("control arm: present (not sealed: older run)"),
            Err(err) if err.downcast_ref::<core::UserError>().is_some() => {
                bad += 1;
                say!("control arm: SEALED CARD MISSING OR CHANGED");
            }
            Err(err) => return Err(err),
        }
    }
    if bad == 0 {
        say!("audit: ok");
    } else {
        say!("audit: {bad} problem(s)");
    }
    Ok(i32::from(bad > 0))
}

/// `council log`: the run's diary, one line per step taken and per seat called.
pub fn cmd_log(run_id: Option<&str>, tail: Option<usize>, as_json: bool) -> Result<i32> {
    let (run, d) = load(run_id)?;
    let mut events = crate::log::events(&d);
    if let Some(n) = tail {
        let skip = events.len().saturating_sub(n);
        events.drain(..skip);
    }
    if as_json {
        for event in &events {
            say!("{}", Json::LINE.dumps(event));
        }
        return Ok(0);
    }
    say!(
        "run {} · {} event(s) · {}",
        run.s("id"),
        events.len(),
        show(&d.join(crate::log::EVENTS))
    );
    for event in &events {
        say!("{}", crate::log::event_line(event));
    }
    if events.is_empty() {
        say!("(nothing recorded yet: steps are written here as they are taken)");
    }
    Ok(0)
}

/// Where a run stands, as data: its step, its loop, the seats of the step and whether each has
/// written its output, and what to do now. For callers that would otherwise read the printed text.
pub fn snapshot(run_id: Option<&str>) -> Option<Value> {
    let (run, d) = load(run_id).ok()?;
    let phase = run.s("phase");
    let seated = matches!(
        phase,
        "research" | "ideate" | "vote" | "rank" | "redteam" | "elders"
    );
    // the loop's folder is made by the step that needs it: looking must not create it
    let files = if seated && d.join(format!("loop-{}", run.show("loop"))).is_dir() {
        seat_files(&run, &d).unwrap_or_default()
    } else {
        Vec::new()
    };
    let owed: Vec<Value> = files
        .iter()
        .map(|f| {
            let seat = run.at("seats").at(&f.seat);
            json!({"seat": f.seat, "role": seat.at("role"), "via": seat.at("via"), "agent": agent(seat),
                   "prompt": show(&f.prompt), "output": show(&f.out),
                   "state": if f.out.exists() { "written" } else { "missing" }})
        })
        .collect();
    let missing = owed.iter().filter(|s| s.s("state") == "missing").count();
    let at = run.show("loop");
    let next = match phase {
        "direct" => "do the work directly, then ingest to close the run".to_string(),
        "brief" => "write brief.yaml in the run folder (next shows the template), then ingest".to_string(),
        "merge" => format!("review loop-{at}/pool.json and set \"reviewed\": true, then ingest"),
        "fuse" => format!("answer every finding in loop-{at}/candidates.json, then ingest"),
        "cut" => "cut (pass eli5: one line on what happened in this loop)".to_string(),
        "final" => "write final.md (replace every TODO), then ingest".to_string(),
        "retro" => "write retro.md (replace every TODO), then ingest".to_string(),
        "done" => "nothing: the run is finished".to_string(),
        _ if owed.is_empty() => "next (it writes this step's packets and lists the seats)".to_string(),
        _ if missing > 0 => format!("start the {missing} seat(s) still missing (dispatch runs the CLI and API ones), then ingest"),
        _ => "ingest".to_string(),
    };
    Some(
        json!({"id": run.at("id"), "tier": run.at("tier"), "phase": phase, "loop": run.int("loop").unwrap_or(0),
                "max_loops": run.int("max_loops").unwrap_or(0), "mode": run.at("mode"), "profile": run.at("profile"),
                "dir": show(&d), "done": phase == "done", "owed": owed, "next": next}),
    )
}
