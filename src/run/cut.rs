//! `council cut`: close a loop. The standing decides who survives; the ledger records why.

use super::*;

/// Count how often each value of a field occurs, in first-seen order.
fn counted<'a>(values: impl Iterator<Item = &'a Value>) -> IndexMap<String, usize> {
    let mut out: IndexMap<String, usize> = IndexMap::new();
    for v in values {
        *out.entry(py::to_str(v)).or_insert(0) += 1;
    }
    out
}

/// How many items the lists of a `{seat: [items]}` record hold in all.
fn total(record: &Value) -> usize {
    items(record)
        .map(|(_, v)| v.as_array().map(Vec::len).unwrap_or(0))
        .sum()
}

/// `", ".join(list)`.
fn commas(value: &Value) -> String {
    crate::val::strings(value).join(", ")
}

/// The ledger of loop `k` (1-based) as Markdown.
pub(crate) fn render_ledger(run: &Value, k: usize) -> String {
    let lp = run
        .list("loops")
        .get(k.saturating_sub(1))
        .unwrap_or(&Value::Null);
    let titles = titles_of(lp);
    let red_team = lp.list("red_team");
    let sev = counted(red_team.iter().map(|f| f.at("severity")));
    let acts = counted(
        red_team
            .iter()
            .filter(|f| f.truthy("response"))
            .map(|f| f.at("response").at("action")),
    );
    let n = |key: &str| sev.get(key).copied().unwrap_or(0);
    let mut md = vec![
        format!("# Loop {k} ledger · {}", run.s("id")),
        String::new(),
        format!("ELI5: {}", lp.get("eli5").map(py::to_str).unwrap_or_default()),
        String::new(),
        format!(
            "- Ideas: {} from {} experts -> {} kept -> pool of {} -> {} to the red team -> {} fused",
            total(lp.at("ideas")),
            items(lp.at("ideas")).count(),
            total(lp.at("kept")),
            lp.list("pool").len(),
            lp.list("shortlist").len(),
            lp.list("candidates").len()
        ),
    ];
    if lp.truthy("vote") {
        md.push(format!("- Shortlist vote: {}", vote_line(lp.at("vote"))));
    }
    if lp.truthy("research") {
        let r = lp.at("research");
        let seats = commas(r.at("seats"));
        let left = if r.truthy("left_out") {
            format!("; left out: {}", commas(r.at("left_out")))
        } else {
            String::new()
        };
        let gaps = if r.truthy("gaps") {
            format!(
                "; gaps: {}",
                crate::val::strings(r.at("gaps"))
                    .into_iter()
                    .take(3)
                    .collect::<Vec<_>>()
                    .join("; ")
            )
        } else {
            String::new()
        };
        md.insert(
            4,
            format!(
                "- Research: {}: {} new fact(s) in the shared memory, {} already held{left}{gaps}",
                if seats.is_empty() {
                    "none came back"
                } else {
                    &seats
                },
                r.get("facts_added")
                    .map(py::to_str)
                    .unwrap_or_else(|| "0".to_string()),
                r.get("facts_merged")
                    .map(py::to_str)
                    .unwrap_or_else(|| "0".to_string())
            ),
        );
    }
    let posted: Vec<&Value> = run
        .list("commons")
        .iter()
        .filter(|x| x.int("loop") == Some(k as i64))
        .collect();
    if !posted.is_empty() {
        md.push(format!(
            "- Commons: {} note(s) ({} question(s) for the researchers)",
            posted.len(),
            posted.iter().filter(|x| x.s("to") == "research").count()
        ));
    }
    let responses: Vec<String> = acts.iter().map(|(a, n)| format!("{a} {n}")).collect();
    md.push(format!(
        "- Red team: S1 {}, S2 {}, S3 {}. Responses: {}",
        n("S1"),
        n("S2"),
        n("S3"),
        if responses.is_empty() {
            "none needed".to_string()
        } else {
            responses.join(", ")
        }
    ));
    if lp.has("red_team_skipped") {
        md.push(format!(
            "- RED TEAM DID NOT RUN: {}",
            if lp.truthy("red_team_skipped") {
                lp.show("red_team_skipped")
            } else {
                "no reason recorded".to_string()
            }
        ));
    }
    if lp.truthy("not_carried") {
        md.push(format!(
            "- Shortlisted but not carried into a candidate by the chair: {}",
            commas(lp.at("not_carried"))
        ));
    }
    if lp.truthy("rank") {
        md.push(format!("- Rank vote: {}", vote_line(lp.at("rank"))));
        for r in lp.at("rank").list("ranking") {
            let check = if r.s("check") == "n/a" {
                String::new()
            } else {
                format!(" [check: {}]", r.show("check"))
            };
            md.push(format!(
                "  - {} {:.2}{check}: {}",
                r.show("id"),
                r.num("score").unwrap_or(0.0),
                titles.get(&r.show("id")).cloned().unwrap_or_default()
            ));
        }
    }
    if lp.truthy("protected_elders") {
        md.push(format!(
            "- Also sent to the elders (the vote could not judge it): {}",
            commas(lp.at("protected_elders"))
        ));
    }
    for rv in lp.list("elder_reviews") {
        let mut who = if rv.truthy("stand_in_for") {
            format!("STAND-IN for {}", rv.show("stand_in_for"))
        } else {
            rv.show("model")
        };
        if rv.s("model_source") == "cli default" {
            who.push_str(" (CLI default model)");
        }
        let pref = if rv.s("preferred") == "CONTROL" {
            "the control arm".to_string()
        } else {
            rv.show("preferred")
        };
        let verdicts: Vec<String> = items(rv.at("verdict"))
            .filter(|(c, _)| c.as_str() != "CONTROL")
            .map(|(c, v)| format!("{c} {}", py::to_str(v)))
            .collect();
        md.push(format!(
            "- Elder {} ({who}): prefers {pref}; {}; confidence {}. Fix: {}",
            rv.show("seat"),
            verdicts.join(", "),
            rv.show("confidence"),
            rv.get("key_fix").map(py::to_str).unwrap_or_default()
        ));
    }
    if lp.has("survivors") {
        let gone = commas(lp.at("eliminated"));
        let wildcard = if lp.truthy("wildcard") {
            format!(
                "; wildcard {} (best on {})",
                lp.at("wildcard").show("label"),
                lp.at("wildcard").show("criterion")
            )
        } else {
            String::new()
        };
        md.push(format!(
            "- Cut: kept {}; eliminated {}{wildcard}",
            commas(lp.at("survivors")),
            if gone.is_empty() { "none" } else { &gone }
        ));
        if !lp.at("control_votes").is_null() && lp.truthy("elder_reviews") {
            md.push(format!(
                "- Control arm: preferred by {} of {} elder(s)",
                lp.show("control_votes"),
                lp.list("elder_reviews").len()
            ));
        }
    }
    for (key, label) in [
        ("asked", "Asked the requester"),
        (
            "unattended",
            "Unattended at a judgment point; safer option taken",
        ),
        ("changed", "What changed"),
        ("stopped_early", "Stopped early"),
    ] {
        if lp.truthy(key) {
            md.push(format!("- {label}: {}", lp.show(key)));
        }
    }
    format!("{}\n", md.join("\n"))
}

/// `council cut`: keep the survivors of this loop, write its ledger and open the next loop (or the final).
pub fn cmd_cut(
    run_id: Option<&str>,
    eli5: &str,
    changed: &str,
    stop: Option<&str>,
    asked: Option<&str>,
    unattended: Option<&str>,
) -> Result<i32> {
    let (mut run, d) = load(run_id)?;
    if run.s("phase") != "cut" {
        return fail(format!(
            "not at the cut yet (phase {}). Run `council next`.",
            run.show("phase")
        ));
    }
    if !(1..=40).contains(&py::words(eli5)) {
        return fail("--eli5 must be one plain line (40 words at most)");
    }
    fn given(text: Option<&str>) -> Option<&str> {
        text.filter(|t| !t.is_empty())
    }
    let (stop, asked, unattended) = (given(stop), given(asked), given(unattended));
    let st = standing(&run);
    let was_last = is_last(&run);
    let last = was_last || stop.is_some();
    let why = judgment_points(&st, last);
    if !why.is_empty() && asked.is_none() && unattended.is_none() {
        return fail(format!(
            "this cut is a judgment point ({}). Say how it was settled:\n  \
             --asked \"<your one question> -> <the answer>\"         if the requester can be asked\n  \
             --unattended \"<the safer option you are taking>\"      if not (nothing irreversible)",
            why.join("; ")
        ));
    }
    let lp = cur(&run).clone();
    let keep = if last {
        1
    } else {
        run.list("schedule")
            .get(loop_no(&run))
            .and_then(Value::as_u64)
            .unwrap_or(1) as usize
    };
    let weights: Map<String, Value> = run
        .at("brief")
        .list("rubric")
        .iter()
        .map(|x| (x.show("id"), x.at("weight").clone()))
        .collect();
    let mut failed = failed_checks(&lp);
    failed.extend(st.kills.iter().cloned());
    let res = core::cut(
        &st.order,
        keep,
        &json!({}),
        &failed,
        lp.at("rank").at("best_on"),
        Some(&Value::Object(weights)),
    );
    let (mut survivors, mut eliminated) = (
        crate::val::strings(res.at("survivors")),
        crate::val::strings(res.at("eliminated")),
    );
    let loop_label = run.show("loop");
    if !last {
        // a vote may not cut what it could not judge
        let spared: Vec<String> =
            core::protected(&st.order, keep, lp.at("rank").at("thin"), &failed)
                .into_iter()
                .filter(|c| eliminated.contains(c))
                .collect();
        if !spared.is_empty() {
            eliminated.retain(|c| !spared.contains(c));
            survivors.extend(spared.iter().cloned());
            let note = format!(
                "loop {loop_label} cut: {} survived although ranked below the line: the vote could not judge it against a survivor",
                spared.join(", ")
            );
            push(&mut run, "deviations", json!(note));
        }
    }
    let rec = cur_mut(&mut run);
    rec["survivors"] = json!(survivors);
    rec["eliminated"] = json!(eliminated);
    rec["wildcard"] = res.at("wildcard").clone();
    rec["eli5"] = json!(py::strip(eli5));
    rec["changed"] = json!(py::strip(changed));
    rec["control_votes"] = json!(st.control_votes);
    rec["finished"] = json!(now());
    if let Some(asked) = asked {
        rec["asked"] = json!(py::strip(asked));
    }
    if let Some(option) = unattended.filter(|_| !why.is_empty()) {
        let option = py::strip(option);
        rec["unattended"] = json!(option);
        let note =
            format!("loop {loop_label} cut: {}, and the requester could not be asked; safer option taken: {option}", why.join("; "));
        push(&mut run, "deviations", json!(note));
    }
    if let Some(reason) = stop.filter(|_| !was_last) {
        let reason = py::strip(reason);
        cur_mut(&mut run)["stopped_early"] = json!(reason);
        if !st.all_ship_leader || !st.s1.is_empty() {
            let note = format!("loop {loop_label} cut: stopped early ({reason}) although the elders did not all say ship");
            push(&mut run, "deviations", json!(note));
        }
    }
    let ledger = loop_dir(&run, &d)?.join("ledger.md");
    core::write_text(&ledger, &render_ledger(&run, loop_no(&run)))?;
    let wildcard = res.at("wildcard");
    say!(
        "cut: kept {}; eliminated {}{}",
        survivors.join(", "),
        if eliminated.is_empty() {
            "none".to_string()
        } else {
            eliminated.join(", ")
        },
        if py::truthy(wildcard) {
            format!(
                "; wildcard {} (best on {})",
                wildcard.show("label"),
                wildcard.show("criterion")
            )
        } else {
            String::new()
        }
    );
    say!("ledger: {}", show(&ledger));
    if last {
        run["winner"] = json!(survivors.first());
        let (n, c, ctrl) = (st.elders, st.control_votes, run.at("control").clone());
        let (result, line) = if !ctrl.truthy("done") || n == 0 {
            let lost = if ctrl.truthy("dropped") {
                ctrl.show("dropped")
            } else if ctrl.truthy("lost") {
                ctrl.show("lost")
            } else {
                String::new()
            };
            (
                "not run",
                if lost.is_empty() {
                    "not run".to_string()
                } else {
                    format!("not run ({lost})")
                },
            )
        } else if 2 * c > n {
            ("lost", format!("THE ONE-SHOT BASELINE WON: {c} of {n} elder(s) preferred it to the council's answer"))
        } else if 2 * c == n {
            (
                "tie",
                format!("tie: {c} of {n} elder(s) preferred the one-shot baseline"),
            )
        } else {
            (
                "won",
                format!(
                    "the council's answer was preferred by {} of {n} elder(s)",
                    n - c
                ),
            )
        };
        run["control_result"] = json!(result);
        run["control_line"] = json!(line);
        run["phase"] = json!("final");
        for e in crate::val::strings(core::check_run(&run).at("errors")) {
            // a broken rule must reach the write-up, not vanish
            push(
                &mut run,
                "deviations",
                json!(format!("protocol check: {e}")),
            );
        }
    } else {
        run["loop"] = json!(loop_no(&run) + 1);
        push(&mut run, "loops", json!({}));
        let (due, _) = support_due(&mut run, &d);
        let research = !due.is_empty();
        cur_mut(&mut run)["research_seats"] = json!(due);
        run["phase"] = json!(if research { "research" } else { "ideate" });
    }
    save(&mut run, &d)?;
    say!();
    let id = run.s("id").to_string();
    cmd_next(Some(&id), None)
}
