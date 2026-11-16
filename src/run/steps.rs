//! `council init` and `council next`: start a run, and say what the current step needs.
//!
//! `next` is safe to run again: it rewrites the packets of the step and prints the same commands.

use super::*;
use crate::gate::{self, GateArgs};

// ----------------------------------------------------------------------------- init

/// Where this run's shared memory lives. It is fixed for the whole run, so a change of settings
/// half-way cannot split one run's notes over two places.
fn pin_memory(root: &Path, obsidian: bool) -> Result<(Value, Vec<String>)> {
    let conf = memory::settings(Some(root));
    if obsidian && conf.s("backend") != "obsidian" {
        return fail(
            "--obsidian needs a vault. Set it once (it is saved in your own settings, not in the repo):\n  \
             council memory setup --obsidian --vault \"<path to your vault>\"",
        );
    }
    if conf.truthy("ok") {
        return Ok((conf, Vec::new()));
    }
    if obsidian {
        return fail(conf.show("problem"));
    }
    let dir = root.join(".council").join("memory").display().to_string();
    let notes = vec![
        conf.show("problem"),
        format!("this run keeps its memory in {dir}. Bring it into the vault later: council memory import --from {dir}"),
    ];
    Ok((
        json!({"backend": "files", "dir": dir, "links": "markdown", "source": "fallback", "ok": true, "problem": null}),
        notes,
    ))
}

/// `council init`: run the gate, seat the council and write the run's first state.
pub fn cmd_init(ask: &str, args: &GateArgs, obsidian: bool) -> Result<i32> {
    let ask = py::strip(ask);
    if py::words(ask) < 3 {
        return fail("the ask is too short. Pass the requester's own words, verbatim");
    }
    let plan = gate::plan(ask, args);
    let root = core::project_root();
    let tier = plan.s("tier").to_string();
    let (mut conf, mut mem_notes) = (Value::Null, Vec::new());
    if tier != "direct" {
        // everything that can refuse the run happens before a folder exists
        if plan.at("models").has("error") {
            return fail(plan.at("models").show("error"));
        }
        (conf, mem_notes) = pin_memory(&root, obsidian)?;
    }
    let rid = format!(
        "{}-{}",
        crate::clock::now().format("%y%m%d-%H%M%S"),
        slug(ask, 3)
    );
    let d = runs_dir(&root).join(&rid);
    if !crate::safe::make_new_dir(&d)? {
        // run ids are stamped to the second: two runs of the same ask in one second collide
        return fail(format!(
            "a run named {rid} already exists: {}. Wait a second and start again, or go on with that run: council status --run {rid}",
            d.display()
        ));
    }
    let mut run = json!({"id": rid, "created": now(), "ask": ask, "paths": args.paths, "gate": plan.at("gate"), "tier": tier,
                         "mode": plan.at("mode"), "profile": plan.at("profile"), "tripwires": plan.at("tripwires"),
                         "gate_reason": plan.at("reason"), "calls_estimate": plan.at("calls_estimate"), "deviations": [],
                         "friction": [], "calls": {}, "loops": []});
    let support = plan.at("support");
    if tier == "direct" {
        run["phase"] = json!("direct");
    } else {
        let picks = plan.at("models");
        let shape = plan.at("shape");
        run["shape"] = shape.clone();
        run["max_loops"] = shape.at("loops").clone();
        run["schedule"] = shape.at("schedule").clone();
        run["chair"] = picks.at("chair").clone();
        run["support"] = support.clone();
        run["memory"] = conf.clone();
        run["commons"] = json!([]);
        run["seats"] = assign_seats(
            &tier,
            picks,
            &gate::doctor(Some(&root), false),
            &core::policy(),
            Some(support),
        );
        run["loop"] = json!(0);
        run["phase"] = json!("brief");
    }
    save(&mut run, &d)?;
    core::write_text(&runs_dir(&root).join("CURRENT"), &format!("{rid}\n"))?;
    say!("{}", plan.s("gate"));
    let seated = py::truthy(run.at("seats"));
    if seated {
        say!("{}", gate::members_line(support, &tier, Some(&conf)));
    }
    say!("run: {}", show(&d));
    let notes = crate::val::strings(plan.at("environment_notes"))
        .into_iter()
        .chain(mem_notes)
        .chain(crate::val::strings(support.at("problems")));
    for note in notes {
        say!("note: {note}");
    }
    if seated {
        for s in seats_of(&run, "researcher") {
            let seat = run.at("seats").at(&s);
            if seat.s("web") != "yes" {
                let what = if seat.s("web") == "no" {
                    "it cannot open web pages, so its facts will be recall only (marked as not re-checked)."
                } else {
                    "whether it can open web pages depends on that tool's own settings. Facts it only recalls are marked."
                };
                say!("note: researcher {s} runs via {}: {what}", seat.show("via"));
            }
        }
    }
    let mut seats: Vec<(&String, &Value)> = items(run.at("seats")).collect();
    seats.sort_by(|a, b| a.0.cmp(b.0));
    for (s, v) in &seats {
        if v.truthy("stand_in_for") {
            say!("note: seat {s} is a STAND-IN for {} (not reachable here). Label it in every artifact.", v.show("stand_in_for"));
        }
    }
    let mut clis: Vec<&str> = seats
        .iter()
        .filter_map(|(_, v)| v.s("via").strip_prefix("cli:"))
        .map(|c| c.split(':').next().unwrap_or(c))
        .collect();
    clis.sort_unstable();
    clis.dedup();
    if !clis.is_empty() {
        say!(
            "note: some seats run through the {} CLI. An installed CLI can still be logged out: check once with `council doctor --probe`.",
            clis.join(", ")
        );
    }
    say!();
    self::cmd_next(Some(&rid), None)
}

// ----------------------------------------------------------------------------- next

/// `council next`: print what the current step needs. Safe to run again.
pub fn cmd_next(run_id: Option<&str>, without_control: Option<&str>) -> Result<i32> {
    let (mut run, d) = load(run_id)?;
    if let Some(what) = without_control {
        if !run.at("control").truthy("done") {
            return fail("this run has no sealed control arm to give up");
        }
        if py::words(what) < 3 {
            return fail(
                "say what happened to the control arm: --without-control \"<what happened>\"",
            );
        }
        let what = py::strip(what);
        run["control"] = json!({"done": false, "lost": what});
        push(
            &mut run,
            "deviations",
            json!(format!(
                "the control arm was lost after it was sealed: {what}"
            )),
        );
    }
    let phase = run.s("phase").to_string();
    if !PHASES.contains(&phase.as_str()) {
        return fail(format!("unknown phase {}", py::repr(run.at("phase"))));
    }
    let mut head = format!("[{}] tier={} phase={phase}", run.s("id"), run.show("tier"));
    if run.truthy("loop") {
        head.push_str(&format!(
            " loop={}/{} calls={}/~{}",
            run.show("loop"),
            run.show("max_loops"),
            calls_so_far(&run),
            run.show("calls_estimate")
        ));
    }
    say!("{head}");
    let lines = match phase.as_str() {
        "direct" => next_direct(),
        "brief" => next_brief(&run, &d)?,
        "research" => next_research(&mut run, &d)?,
        "ideate" => next_ideate(&mut run, &d)?,
        "merge" => next_merge(&run, &d)?,
        "vote" => next_vote(&run, &d)?,
        "redteam" => next_redteam(&run, &d)?,
        "fuse" => next_fuse(&run, &d)?,
        "rank" => next_rank(&run, &d)?,
        "elders" => next_elders(&mut run, &d)?,
        "cut" => next_cut(&run)?,
        "final" => next_final(&run, &d)?,
        "retro" => next_retro(&run, &d)?,
        _ => next_done(&d),
    };
    for line in lines {
        say!("{line}");
    }
    save(&mut run, &d)?;
    Ok(0)
}

/// Every phase of a run, in order.
pub(crate) const PHASES: [&str; 14] = [
    "direct", "brief", "research", "ideate", "merge", "vote", "redteam", "fuse", "rank", "elders",
    "cut", "final", "retro", "done",
];

/// Seat calls recorded so far, over every step.
pub(crate) fn calls_so_far(run: &Value) -> i64 {
    items(run.at("calls")).filter_map(|(_, n)| n.as_i64()).sum()
}

fn next_direct() -> Vec<String> {
    [
        "Direct tier: one writer, tests first.",
        "  1. Have a different agent write the acceptance tests (or mutation-check yours) BEFORE the code.",
        "  2. Lock them:      council lock-tests <test paths>",
        "  3. Implement, then loop: run the checks, fix, repeat. 5 iterations at most.",
        "     Still failing after 5, or a real design fork appeared: raise the tier (council init ... --tier lite).",
        "  4. Prove the locks:  council verify-locks",
        "THEN  council ingest        # closes the run",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect()
}

fn next_brief(run: &Value, d: &Path) -> Result<Vec<String>> {
    let f = d.join("brief.yaml");
    let root = core::project_root();
    if !f.exists() {
        let rubric = run.at("shape").list("rubric");
        let bound = |i: usize| rubric.get(i).map(py::to_str).unwrap_or_default();
        let stubs: Vec<String> = seats_of(run, "expert")
            .iter()
            .map(|s| format!("  - {{id: {s}, name: \"\", maximize: \"\", respect: \"\", method: \"\", evidence: \"\"}}"))
            .collect();
        let ask: Vec<String> = py::lines(run.s("ask"))
            .iter()
            .map(|l| format!("#   {l}"))
            .collect();
        let vars = crate::vars!(
            run_id = run.s("id"),
            rubric_min = bound(0),
            rubric_max = bound(1),
            lens_stubs = stubs.join("\n"),
            ask_commented = ask.join("\n")
        );
        core::write_text(&f, &tpl("brief.yaml", &vars)?)?;
    }
    let mut out = vec![
        format!("Write the intent brief: {}", show(&f)),
        "  Rubric first, before any ideas. Lenses by objective + constraint + method, not job titles.".to_string(),
    ];
    let ls = lessons(&root);
    if !ls.is_empty() {
        out.push(
            "  Lessons to weigh (docs/lessons.md); list the ones you use under lessons_applied:"
                .to_string(),
        );
        for x in ls.iter().take(7) {
            out.push(format!(
                "    {} (+{}/-{}): {}",
                x.s("id"),
                x.show("helpful"),
                x.show("harmful"),
                py::head(x.s("text"), 150)
            ));
        }
    }
    let n = seats_of(run, "researcher").len();
    if n > 0 {
        out.push(format!(
            "  research_questions: what the {n} researcher{} should find out before anyone has ideas \
             (6 at most; name things in full, no acronyms).",
            if n > 1 { "s" } else { "" }
        ));
        let dg = memory::digest(run.s("ask"), None, 6, &memory_conf(run, &root));
        if !dg.list("lines").is_empty() {
            let total = if dg.has("total") {
                dg.show("total")
            } else {
                "0".to_string()
            };
            out.push(format!("  Already in the research memory ({total} notes; `council memory search \"<words>\"`):"));
            out.extend(dg.list("lines").iter().map(|line| {
                format!(
                    "    {}",
                    py::to_str(line).chars().skip(2).collect::<String>()
                )
            }));
        }
    }
    out.push(
        "  If the requester is here, show them the brief (one screen) and set approved_by to their name \
         (or \"requester\"). Otherwise: unattended."
            .to_string(),
    );
    out.push("THEN  council ingest".to_string());
    Ok(out)
}

/// The run's pinned memory settings, or the project's when the run has none.
pub(crate) fn memory_conf(run: &Value, root: &Path) -> Value {
    if run.truthy("memory") {
        run.at("memory").clone()
    } else {
        memory::settings(Some(root))
    }
}

/// Write the skeleton and return (the skeleton, what the mapper's packet needs to know about the map so far).
fn map_packet_vars(root: &Path, skeleton_file: &Path) -> Result<(codemap::Skeleton, Vars)> {
    let budget = core::members(root, &core::MemberFlags::default())
        .at("map")
        .int("skeleton_lines")
        .unwrap_or(420)
        .max(0) as usize;
    let sk = codemap::skeleton(root, budget, codemap::scan(root));
    core::write_text(skeleton_file, &sk.markdown)?;
    let path = codemap::map_path(root);
    let existing = if !path.exists() {
        format!(
            "There is no map yet. Yours will be saved as `{}`.",
            py::posix(path.strip_prefix(root).unwrap_or(&path))
        )
    } else {
        let text = crate::safe::read_capped(&path, crate::safe::MAX_OUTPUT).unwrap_or_default();
        let text = crate::safe::shown(&text);
        let first = py::lines(&text)
            .into_iter()
            .take(5)
            .collect::<Vec<_>>()
            .join("\n");
        if first.contains(codemap::MARKER) {
            format!(
                "An earlier map is at `{}`. Read it, keep what is still true, and fix what is not.",
                show(&path)
            )
        } else {
            format!(
                "A map written by hand is at `{}`. Read it first. Yours is saved beside it, so do not contradict it without saying why.",
                show(&path)
            )
        }
    };
    let diff = sk.stale.at("diff");
    let moved: usize = ["added", "changed", "removed"]
        .iter()
        .map(|k| diff.list(k).len())
        .sum();
    let changed = if sk.stale.truthy("mapped") && moved > 0 {
        format!("Since the last map, the structure changed in {moved} file(s): start there.")
    } else {
        String::new()
    };
    Ok((
        sk,
        crate::vars!(
            skeleton_path = show(skeleton_file),
            existing_map = existing,
            changed = changed
        ),
    ))
}

/// The mapper without a council: write its packet, so one fresh agent can refresh the map on its own.
pub fn cmd_map_packet() -> Result<i32> {
    let root = core::project_root();
    let md = root.join(".council").join("map");
    let (sk, mut vars) = map_packet_vars(&root, &md.join("skeleton.md"))?;
    let (out, prompt) = (md.join("MAP.out.md"), md.join("MAP.prompt.md"));
    let via = fresh(&gate::doctor(Some(&root), false));
    vars.extend(crate::vars!(
        run_id = "none (stand-alone)",
        loop = "-",
        seat = "MAP",
        ask = "> (no council run: keep the codebase map current)",
        output_rule = output_rule(&json!({"via": "subagent"}), &out)
    ));
    core::write_text(&prompt, &tpl("packets/map.md", &vars)?)?;
    let path = codemap::map_path(&root);
    say!(
        "map: {}: {}",
        py::posix(path.strip_prefix(&root).unwrap_or(&path)),
        sk.stale.s("reason")
    );
    say!(
        "skeleton: {} source files, {} lines",
        sk.stats.code_files,
        py::commas(sk.stats.lines as i64)
    );
    if via.get("via").and_then(Value::as_str) == Some("subagent") {
        say!("SPAWN one fresh-context agent (council-mapper). Its prompt is one line:");
        say!("  prompt: Read and follow {}", show(&prompt));
    } else {
        say!("Give this packet to a fresh context (or do it yourself, reading only what it names): {}", show(&prompt));
    }
    say!("THEN  council map --install {}", show(&out));
    Ok(0)
}

/// The loop before this one.
fn prev_loop(run: &Value) -> &Value {
    run.list("loops")
        .get(loop_no(run).saturating_sub(2))
        .unwrap_or(&Value::Null)
}

/// The cards that survived a loop, in the order they were fused.
fn survivors_of(lp: &Value) -> Vec<&Value> {
    lp.list("candidates")
        .iter()
        .filter(|c| lp.list("survivors").contains(c.at("id")))
        .collect()
}

fn next_research(run: &mut Value, d: &Path) -> Result<Vec<String>> {
    let root = core::project_root();
    let conf = memory_conf(run, &root);
    let seats = crate::val::strings(cur(run).at("research_seats"));
    let researchers: Vec<String> = seats.iter().filter(|x| *x != "MAP").cloned().collect();
    let (qs, asked) = research_questions(run);
    let n_posts = run.list("commons").len();
    cur_mut(run)["research_asked"] = json!(n_posts);
    let mut q_lines: Vec<String> = qs
        .iter()
        .enumerate()
        .map(|(i, q)| format!("{}. {q}", i + 1))
        .collect();
    if !asked.is_empty() {
        if !q_lines.is_empty() {
            q_lines.push(String::new());
        }
        q_lines.push("Asked by council members (answer these first):".to_string());
        let waiting = said(open_requests(run));
        q_lines.extend(
            waiting[waiting.len().saturating_sub(8)..]
                .iter()
                .map(|t| format!("- {t}")),
        );
    }
    if loop_no(run) > 1 {
        let titles: Vec<String> = survivors_of(prev_loop(run))
            .iter()
            .map(|c| c.show("title"))
            .collect();
        q_lines.push(String::new());
        q_lines.push(format!(
            "The council is now refining: {}.",
            titles.join("; ")
        ));
    }
    let dg = memory::digest(&about_text(run), Some(run.s("id")), 12, &conf);
    let known = if dg.list("lines").is_empty() {
        "Nothing on this topic yet.".to_string()
    } else {
        format!(
            "{}\nDo not find these again. Re-check one only if the decision rests on it. One note per fact: `{}`.",
            crate::val::strings(dg.at("lines")).join("\n"),
            dg.show("dir")
        )
    };
    let ran: Vec<String> = run
        .list("loops")
        .iter()
        .flat_map(|x| x.at("research").list("queries").iter().map(py::to_str))
        .collect();
    let searched = if ran.is_empty() {
        String::new()
    } else {
        format!(
            "\nSearches already run in this council (do not repeat them): {}\n",
            ran.iter().take(40).cloned().collect::<Vec<_>>().join("; ")
        )
    };
    let support = sup(run);
    let budget = support
        .at("research")
        .at("budget")
        .get(run.s("tier"))
        .map(py::to_str)
        .unwrap_or_else(|| "10".to_string());
    let lens_names: Vec<String> = run
        .at("brief")
        .list("lenses")
        .iter()
        .map(|l| l.show("name"))
        .collect();
    let mut common = common_vars(run, d);
    common.extend(crate::vars!(
        questions = q_lines.join("\n"),
        known = known,
        searched = searched,
        lens_names = lens_names.join(", "),
        budget = budget
    ));
    let frozen: &Value = run;
    let beat = |x: &str| -> Result<Vars> {
        let rest: Vec<String> = seats_of(frozen, "researcher")
            .iter()
            .filter(|o| *o != x)
            .map(|o| frozen.at("seats").at(o).show("beat").to_lowercase())
            .collect();
        let others = if rest.is_empty() {
            "You are the only researcher in this run.".to_string()
        } else {
            let many = rest.len() > 1;
            format!(
                "The other researcher{}: {}. Leave {} to them.",
                if many { "s cover" } else { " covers" },
                rest.join("; "),
                if many { "those" } else { "that" }
            )
        };
        let seat = frozen.at("seats").at(x);
        Ok(crate::vars!(
            beat_name = seat.show("beat"),
            beat_text = seat.show("beat_text"),
            others = others
        ))
    };
    if !researchers.is_empty() {
        write_seat_prompts(
            run,
            d,
            "research.md",
            &common,
            Some(&beat),
            Some(&researchers),
        )?;
    }
    let mut out: Vec<String> = Vec::new();
    if seats.iter().any(|s| s == "MAP") {
        let (sk, extra) = map_packet_vars(
            &root,
            &loop_dir(run, d)?.join("research").join("skeleton.md"),
        )?;
        let mut vars = common.clone();
        vars.extend(extra);
        write_seat_prompts(run, d, "map.md", &vars, None, Some(&["MAP".to_string()]))?;
        let path = codemap::map_path(&root);
        out.push(format!(
            "Mapper: {} source files, {} lines. The map goes to {} ({}).",
            sk.stats.code_files,
            py::commas(sk.stats.lines as i64),
            py::posix(path.strip_prefix(&root).unwrap_or(&path)),
            sk.stale.s("reason")
        ));
    }
    start_loop(run);
    if !researchers.is_empty() {
        let beats: Vec<String> = researchers
            .iter()
            .map(|x| run.at("seats").at(x).show("beat").to_lowercase())
            .collect();
        out.insert(
            0,
            format!(
                "Research: {} researcher(s) ({}), {} question(s). They bring back sourced facts. They do not propose and do not vote.",
                researchers.len(),
                beats.join("; "),
                qs.len() + asked.len()
            ),
        );
        out.insert(
            1,
            format!(
                "  Facts are filed in the shared memory: {}",
                conf.show("dir")
            ),
        );
    }
    out.extend(seat_step(run, d)?);
    Ok(out)
}

/// Record when the loop's first seat step began (once).
fn start_loop(run: &mut Value) {
    if !cur(run).has("started") {
        cur_mut(run)["started"] = json!(now());
    }
}

fn next_ideate(run: &mut Value, d: &Path) -> Result<Vec<String>> {
    let mut vars = common_vars(run, d);
    let shape = run.at("shape").clone();
    let (mut survivors_block, mut mutation_rule) = (String::new(), String::new());
    if loop_no(run) > 1 {
        let prev = prev_loop(run);
        let lines: Vec<String> = survivors_of(prev)
            .iter()
            .map(|c| format!("- **{}**: {}", c.show("title"), c.show("body")))
            .collect();
        let reviews = prev.list("elder_reviews");
        let mut fb: Vec<String> = reviews
            .iter()
            .filter(|r| r.truthy("own_idea"))
            .map(|r| format!("- {}", r.show("own_idea")))
            .collect();
        fb.extend(
            reviews
                .iter()
                .filter(|r| r.truthy("key_fix"))
                .map(|r| format!("- Fix asked for: {}", r.show("key_fix"))),
        );
        let feedback = if fb.is_empty() {
            String::new()
        } else {
            format!("\n\n## Elder feedback to use\n{}", fb.join("\n"))
        };
        survivors_block = format!(
            "\n## Survivors of the last loop\n{}{feedback}\n",
            lines.join("\n")
        );
        mutation_rule =
            "\n   This is a later loop: every idea must improve, graft, simplify or invert the survivors above. Do not start over.".to_string();
    }
    let common = vars.clone();
    vars.extend(crate::vars!(
        ideas_min = shape.show("ideas_min"),
        keep = shape.show("keep"),
        survivors_block = survivors_block,
        mutation_rule = mutation_rule,
        memory_block = memory_block(run, d, true, true)?,
        share_rule = share_rule(run)
    ));
    let frozen: &Value = run;
    write_seat_prompts(
        run,
        d,
        "ideate.md",
        &vars,
        Some(&|s: &str| lens_vars(frozen, s)),
        None,
    )?;
    if wants_control(run) {
        let cd = d.join("control");
        let mut vars = common;
        vars.extend(crate::vars!(
            seat = "CTRL",
            memory_block = memory_block(run, d, false, true)?, // the same facts, no thoughts
            output_rule = output_rule(run.at("seats").at("CTRL"), &cd.join("CTRL.out.json"))
        ));
        core::write_text(
            &cd.join("CTRL.prompt.md"),
            &tpl("packets/control.md", &vars)?,
        )?;
    }
    start_loop(run);
    let mut out = vec![format!(
        "Ideate: {} experts, each alone, at least {} ideas, keep {}.",
        seats_of(run, "expert").len(),
        shape.show("ideas_min"),
        shape.show("keep")
    )];
    if wants_control(run) {
        out.push("Seat CTRL is the control arm: one fresh one-shot answer. Do not read its output; the tool blinds it for the elders.".to_string());
    }
    out.extend(seat_step(run, d)?);
    Ok(out)
}

/// Every card the experts kept this loop, with where it came from (`L2#1` = seat L2's first card).
pub(crate) fn kept_cards(run: &Value) -> Vec<Value> {
    let kept = cur(run).at("kept");
    let mut seats: Vec<&String> = items(kept).map(|(s, _)| s).collect();
    seats.sort();
    let mut out = Vec::new();
    for s in seats {
        for (i, c) in kept.list(s).iter().enumerate() {
            let mut card = json!({"ref": format!("{s}#{}", i + 1), "seat": s});
            for (k, v) in items(c) {
                card[k] = v.clone();
            }
            out.push(card);
        }
    }
    out
}

/// A list field as one line: its items as text, or the value itself when it is not a list.
pub(crate) fn joined(value: &Value) -> String {
    match value {
        Value::Array(a) => a.iter().map(py::to_str).collect::<Vec<_>>().join(", "),
        other if py::truthy(other) => py::to_str(other),
        _ => String::new(),
    }
}

fn next_merge(run: &Value, d: &Path) -> Result<Vec<String>> {
    let ld = loop_dir(run, d)?;
    let cards = kept_cards(run);
    let tokens: Vec<HashSet<String>> = cards
        .iter()
        .map(|c| card_tokens(&format!("{} {}", c.show("title"), c.show("body"))))
        .collect();
    let mut pairs: Vec<(f64, String, String)> = Vec::new();
    for i in 0..cards.len() {
        for j in i + 1..cards.len() {
            let both = tokens[i].intersection(&tokens[j]).count();
            let either = tokens[i].union(&tokens[j]).count().max(1);
            let overlap = both as f64 / either as f64;
            if overlap >= 0.25 {
                pairs.push((overlap, cards[i].show("ref"), cards[j].show("ref")));
            }
        }
    }
    let mut md = vec![
        format!(
            "# Kept cards, loop {} ({} cards)",
            run.show("loop"),
            cards.len()
        ),
        String::new(),
    ];
    if !pairs.is_empty() {
        pairs.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
        md.push("Likely overlaps (word overlap; your judgment decides):".to_string());
        md.extend(
            pairs
                .iter()
                .take(12)
                .map(|(j, a, b)| format!("- {a} ~ {b} ({:.0}%)", j * 100.0)),
        );
        md.push(String::new());
    }
    for c in &cards {
        md.push(format!("## {}: {}", c.show("ref"), c.show("title")));
        md.push(c.show("body"));
        md.push(format!(
            "Wins: {} · Evidence: {}",
            joined(c.at("wins")),
            c.get("evidence").map(py::to_str).unwrap_or_default()
        ));
        md.push(format!("ELI5: {}", c.show("eli5")));
        md.push(String::new());
    }
    core::write_text(&ld.join("cards.md"), &md.join("\n"))?;
    let pool = ld.join("pool.json");
    if !pool.exists() {
        let drafts: Vec<Value> = cards
            .iter()
            .enumerate()
            .map(|(i, c)| json!({"id": format!("P{}", i + 1), "title": c.at("title"), "body": c.at("body"), "eli5": c.at("eli5"), "from": [c.at("ref")]}))
            .collect();
        core::write_text(
            &pool,
            &Json::ONE.dumps(&json!({"reviewed": false, "pool": drafts})),
        )?;
    }
    Ok(vec![
        format!("Merge duplicates into idea families. Read {}", show(&ld.join("cards.md"))),
        format!("  Edit {} (a draft with every kept card is already there):", show(&pool)),
        "  - Merge cards that are the same idea into one card. \"from\" = every kept card it came from, e.g. [\"L1#2\", \"L4#1\"].".to_string(),
        "    The tool works out the authors from \"from\"; every kept card must appear in some pool card.".to_string(),
        "  - Keep distinct directions, including ones you do not like. You merge; you do not judge.".to_string(),
        "  - Keep each body 90 to 110 words and free of seat ids and \"my lens\" talk. Set \"reviewed\": true.".to_string(),
        "THEN  council ingest".to_string(),
    ])
}

/// Blind the cards for a step, keep who-wrote-what away from the seats, and write the packet.
fn blind_into(run: &Value, d: &Path, phase: &str, cards: &[Value]) -> Result<Value> {
    let res = core::blind(
        cards,
        seed_for(run, phase),
        core::caps::CARD,
        &seats_of(run, "expert"),
        SPREAD,
    );
    if !res.truthy("ok") {
        return fail(format!(
            "cannot blind the cards:\n  {}",
            crate::val::strings(res.at("errors")).join("\n  ")
        ));
    }
    let mut private_data = private(run, d);
    private_data[format!("{}/{phase}", run.show("loop"))] = res.at("mapping").clone();
    save_private(run, d, &private_data)?;
    core::write_text(
        &loop_dir(run, d)?.join(phase).join("packet.md"),
        res.s("packet"),
    )?;
    Ok(res)
}

/// Which card is behind which label in a step's packet (label -> `{"id", "authors"}`).
pub(crate) fn mapping_for(run: &Value, d: &Path, phase: &str) -> Result<Value> {
    let mapping = private(run, d)
        .at(&format!("{}/{phase}", run.show("loop")))
        .clone();
    if !py::truthy(&mapping) {
        return fail(format!(
            "private mapping for {phase} is missing ({}). Re-run `council next`.",
            private_file(run, d).display()
        ));
    }
    Ok(mapping)
}

fn checks_block(run: &Value, mapping: &Value) -> String {
    let checks = cur(run).at("checks");
    let rows: Vec<String> = items(mapping)
        .filter_map(|(l, m)| match checks.at(&m.show("id")).as_str() {
            Some(result @ ("pass" | "fail")) => Some(format!("- Candidate {l}: {result}")),
            _ => None,
        })
        .collect();
    if rows.is_empty() {
        "\n## Machine checks\nNone were run on these candidates. Where the rubric says \"machine\", judge from the \
         evidence a candidate gives, and count it against a candidate that gives none.\n"
            .to_string()
    } else {
        format!(
            "\n## Machine checks (a fail outranks any vote)\n{}\n",
            rows.join("\n")
        )
    }
}

/// The labels of a blinded packet, in packet order.
fn labels_of(res: &Value) -> Vec<String> {
    res.list("cards").iter().map(|c| c.show("label")).collect()
}

fn vote_prompts(run: &Value, d: &Path, phase: &str, purpose: &str, cards: &[Value]) -> Result<()> {
    let res = blind_into(run, d, phase, cards)?;
    let orders = core::seat_orders(&labels_of(&res), &seats_of(run, "expert"));
    let mut vars = common_vars(run, d);
    vars.extend(crate::vars!(
        purpose = purpose,
        n_cards = cards.len(),
        checks_block = checks_block(run, res.at("mapping"))
    ));
    let per_seat = |s: &str| -> Result<Vars> {
        let mut own = lens_vars(run, s)?;
        own.insert(
            "packet",
            core::render_packet(res.list("cards"), orders.get(s).map(Vec::as_slice)),
        );
        Ok(own)
    };
    write_seat_prompts(run, d, "vote.md", &vars, Some(&per_seat), None)
}

fn next_vote(run: &Value, d: &Path) -> Result<Vec<String>> {
    let pool = cur(run).list("pool");
    vote_prompts(run, d, "vote", "shortlist", pool)?;
    let mut out = vec![format!(
        "Shortlist vote: {} pooled cards, blind, ranked by every expert seat.",
        pool.len()
    )];
    out.extend(seat_step(run, d)?);
    Ok(out)
}

/// The pool cards that reached the red team.
pub(crate) fn shortlisted(lp: &Value) -> Vec<Value> {
    lp.list("pool")
        .iter()
        .filter(|c| lp.list("shortlist").contains(c.at("id")))
        .cloned()
        .collect()
}

fn next_redteam(run: &Value, d: &Path) -> Result<Vec<String>> {
    let cards = shortlisted(cur(run));
    let res = blind_into(run, d, "redteam", &cards)?;
    let mut vars = common_vars(run, d);
    vars.extend(crate::vars!(
        packet = res.s("packet"),
        memory_block = memory_block(run, d, true, true)?,
        share_rule = share_rule(run)
    ));
    write_seat_prompts(run, d, "redteam.md", &vars, None, None)?;
    let mut out = vec![format!(
        "Red team: pre-mortem on the {} shortlisted cards.",
        cards.len()
    )];
    out.extend(seat_step(run, d)?);
    Ok(out)
}

/// `d.get(key, "")` shown as text.
fn shown_or_empty(d: &Value, key: &str) -> String {
    d.get(key).map(py::to_str).unwrap_or_default()
}

fn next_fuse(run: &Value, d: &Path) -> Result<Vec<String>> {
    let (lp, ld, n) = (cur(run), loop_dir(run, d)?, n_fuse(run));
    let cards = shortlisted(lp);
    let score: HashMap<String, f64> = lp
        .at("vote")
        .list("ranking")
        .iter()
        .filter_map(|r| r.num("score").map(|s| (r.show("id"), s)))
        .collect();
    let want = n.min(cards.len());
    let mut md = vec![
        format!(
            "# Fuse input, loop {}: make exactly {want} candidates",
            run.show("loop")
        ),
        String::new(),
    ];
    for c in &cards {
        let voted = score
            .get(&c.show("id"))
            .map(|s| format!(" · shortlist vote {s:.2}"))
            .unwrap_or_default();
        md.push(format!(
            "## {}: {}  (authors {}{voted})",
            c.show("id"),
            c.show("title"),
            crate::val::strings(c.at("authors")).join(", ")
        ));
        md.push(c.show("body"));
        md.push(format!("ELI5: {}", c.show("eli5")));
        md.push(String::new());
    }
    md.push("## Red-team findings (answer every S1 and S2)".to_string());
    let red_team = lp.list("red_team");
    for f in red_team {
        md.push(format!(
            "{}. [{}] {}: {} | evidence: {} | fix: {}",
            f.show("n"),
            f.show("severity"),
            f.show("candidate_id"),
            f.show("finding"),
            shown_or_empty(f, "evidence"),
            shown_or_empty(f, "fix")
        ));
    }
    if red_team.is_empty() {
        md.push("(none recorded)".to_string());
    }
    if loop_no(run) > 1 {
        md.push(String::new());
        md.push("## Elder feedback from the last loop".to_string());
        for r in prev_loop(run).list("elder_reviews") {
            md.push(format!(
                "- {}: fix: {} | idea: {}",
                r.show("seat"),
                shown_or_empty(r, "key_fix"),
                shown_or_empty(r, "own_idea")
            ));
        }
    }
    let found = run
        .list("loops")
        .iter()
        .rev()
        .map(|x| x.at("research"))
        .find(|r| py::truthy(r));
    if let Some(found) = found.filter(|f| f.truthy("synthesis") || f.truthy("gaps")) {
        md.push(String::new());
        md.push("## What the researchers found (facts are in the shared memory)".to_string());
        md.extend(items(found.at("synthesis")).map(|(k, v)| {
            format!(
                "- {}: {}",
                run.at("seats").at(k).show("beat"),
                py::to_str(v)
            )
        }));
        md.extend(
            found
                .list("conflicts")
                .iter()
                .take(6)
                .map(|c| format!("- Sources disagree: {}", py::to_str(c))),
        );
        md.extend(
            found
                .list("gaps")
                .iter()
                .take(6)
                .map(|g| format!("- Not found: {}", py::to_str(g))),
        );
    }
    let posts: Vec<&Value> = run
        .list("commons")
        .iter()
        .filter(|x| x.at("loop") == run.at("loop"))
        .collect();
    for (to, head) in [("all", "to every member"), ("chair", "to you")] {
        let lines = said(posts.iter().copied().filter(|x| x.s("to") == to));
        if !lines.is_empty() {
            md.push(String::new());
            md.push(format!(
                "## Notes from the council, this loop, {head} (unsigned)"
            ));
            md.extend(lines.iter().map(|t| format!("- {t}")));
        }
    }
    let waiting = said(open_requests(run));
    if !waiting.is_empty() {
        md.push(String::new());
        md.push(format!(
            "## Questions waiting for the researchers{}",
            if !seats_of(run, "researcher").is_empty() && !is_last(run) {
                " (they answer before the next loop)"
            } else {
                " (no research round is left in this run: answer what you can, file what you find with \
                 `council memory add`, and list the rest as open in the final)"
            }
        ));
        md.extend(waiting.iter().map(|t| format!("- {t}")));
    }
    core::write_text(&ld.join("fuse-input.md"), &md.join("\n"))?;
    let out_f = ld.join("candidates.json");
    if !out_f.exists() {
        let mut ranked: Vec<&Value> = cards.iter().collect();
        let key = |c: &Value| -score.get(&c.show("id")).copied().unwrap_or(0.0);
        ranked.sort_by(|a, b| {
            key(a)
                .partial_cmp(&key(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let candidates: Vec<Value> = ranked
            .iter()
            .take(want)
            .enumerate()
            .map(|(i, c)| json!({"id": format!("C{}", i + 1), "title": c.at("title"), "body": c.at("body"), "eli5": c.at("eli5"), "parents": [c.at("id")]}))
            .collect();
        let responses: Vec<Value> = red_team
            .iter()
            .filter(|f| matches!(f.s("severity"), "S1" | "S2"))
            .map(|f| json!({"finding": f.at("n"), "action": "TODO", "note": ""}))
            .collect();
        core::write_text(
            &out_f,
            &Json::ONE
                .dumps(&json!({"candidates": candidates, "responses": responses, "checks": {}})),
        )?;
    }
    Ok(vec![
        format!("Refine and recombine into exactly {want} candidates. Read {}", show(&ld.join("fuse-input.md"))),
        format!("  Edit {} (a draft is there):", show(&out_f)),
        "  - responses: every S1/S2 finding gets action fixed | rebutted | accepted, with a note. A rebuttal needs evidence.".to_string(),
        "  - candidates: fuse the strongest parts. \"parents\" = every pool card it was built from, e.g. [\"P2\", \"P5\"].".to_string(),
        "    The tool works out the authors from the parents (their votes on it are discounted).".to_string(),
        "  - body 90 to 110 words, no seat ids or \"my lens\" talk. Each card needs an ELI5.".to_string(),
        "  - checks: {\"C1\": \"pass\" | \"fail\"} for any candidate you could test by machine. A fail ranks last.".to_string(),
        "  You refine and recombine. You do not rank: the experts do that next, blind.".to_string(),
        "THEN  council ingest".to_string(),
    ])
}

fn next_rank(run: &Value, d: &Path) -> Result<Vec<String>> {
    let candidates = cur(run).list("candidates");
    vote_prompts(run, d, "rank", "final ranking of this loop", candidates)?;
    let mut out = vec![format!(
        "Rank vote: {} fused candidates, blind, ranked by every expert seat.",
        candidates.len()
    )];
    out.extend(seat_step(run, d)?);
    Ok(out)
}

/// A lone elder (Lite) or a max-profile elder checks claims by running code; otherwise elders judge
/// the packet only, which costs about a quarter as much per call.
fn elders_verify(run: &Value) -> bool {
    core::elders_verify(run.s("tier"), run.s("profile"))
}

fn elder_top(run: &Value) -> usize {
    if run.s("tier") == "full" && is_last(run) {
        1
    } else {
        2
    }
}

/// The sealed one-shot baseline, read back from disk. `None` when the run has no control arm.
pub(crate) fn control_card(run: &Value, d: &Path) -> Result<Option<Value>> {
    let c = run.at("control");
    if !c.truthy("done") {
        return Ok(None);
    }
    let f = d.join("control").join("CTRL.out.json");
    let obj = read_out(&f);
    let field = |k: &str| obj.as_ref().map(|o| text_of(o.at(k))).unwrap_or_default();
    let card = json!({"title": field("title"), "body": field("body"), "eli5": field("eli5")});
    if obj.is_none() || (c.truthy("sha256") && sha_value(&card) != c.s("sha256")) {
        return fail(format!(
            "the control arm is sealed, but {} is missing or was changed after it was ingested.\n  \
             Restore the file. If it is gone for good, say so and go on without it:\n  \
             council next --without-control \"<what happened>\"",
            show(&f)
        ));
    }
    Ok(Some(card))
}

/// The candidate ids a loop's machine checks failed.
pub(crate) fn failed_checks(lp: &Value) -> HashSet<String> {
    items(lp.at("checks"))
        .filter(|(_, res)| res.as_str() == Some("fail"))
        .map(|(c, _)| c.clone())
        .collect()
}

fn next_elders(run: &mut Value, d: &Path) -> Result<Vec<String>> {
    let lp = cur(run).clone();
    let order = crate::val::strings(lp.at("rank").at("order"));
    let top = elder_top(run).min(order.len());
    let extra: Vec<String> =
        core::protected(&order, top, lp.at("rank").at("thin"), &failed_checks(&lp))
            .into_iter()
            .take(2)
            .collect();
    if !extra.is_empty() && lp.at("protected_elders") != &json!(extra) {
        cur_mut(run)["protected_elders"] = json!(extra);
        let note = format!(
            "loop {} elders: {} also went to the elders, because the vote could not judge it against {}",
            run.show("loop"),
            extra.join(", "),
            order[..top].join(", ")
        );
        push(run, "deviations", json!(note));
    }
    let reviewed: Vec<String> = order[..top].iter().chain(extra.iter()).cloned().collect();
    let mut cards: Vec<Value> = lp
        .list("candidates")
        .iter()
        .filter(|c| reviewed.contains(&c.show("id")))
        .cloned()
        .collect();
    let ctrl = control_card(run, d)?;
    if let Some(card) = &ctrl {
        cards.push(json!({"id": "CONTROL", "title": card.at("title"), "body": card.at("body"), "eli5": card.at("eli5"), "authors": []}));
    }
    cur_mut(run)["reviewed"] = json!(reviewed);
    let res = blind_into(run, d, "elders", &cards)?;
    let orders = core::seat_orders(&labels_of(&res), &seats_of(run, "elder"));
    let verify = if elders_verify(run) {
        "If you can run code or read project files, reproduce a claim before you rely on it, in a scratch copy \
         (never in the project), and say \"reproduced\" or \"not checked\"."
    } else {
        "Judge from this packet alone: do not open other files. Say \"not checked\" where a claim would need a \
         check you cannot do from here."
    };
    let mut vars = common_vars(run, d);
    vars.extend(crate::vars!(
        checks_block = checks_block(run, res.at("mapping")),
        verify_rule = verify,
        share_rule = share_rule(run),
        memory_block = memory_block(run, d, false, elders_verify(run))?
    ));
    let per_seat = |s: &str| -> Result<Vars> {
        Ok(crate::vars!(
            packet = core::render_packet(res.list("cards"), orders.get(s).map(Vec::as_slice))
        ))
    };
    write_seat_prompts(run, d, "elder.md", &vars, Some(&per_seat), None)?;
    let extra_note = if ctrl.is_some() {
        " plus the blinded control arm"
    } else {
        " (no control arm in this run)"
    };
    let mut out = vec![format!(
        "Elder review: the top {} candidate(s){extra_note}.",
        reviewed.len()
    )];
    if ctrl.is_some() {
        out.push("Do not open the elder packets or outputs yourself: they hold the control arm, which you must not see yet.".to_string());
    }
    out.extend(seat_step(run, d)?);
    Ok(out)
}

/// The rank vote, the machine checks and the elder verdicts, combined into the order used for the cut.
pub(crate) struct Standing {
    pub order: Vec<String>,
    pub kills: Vec<String>,
    pub swapped: bool,
    pub control_votes: usize,
    pub elders: usize,
    pub s1: Vec<Value>,
    pub all_ship_leader: bool,
    pub split: bool,
    pub leader: String,
    pub unreviewed_leader: bool,
}

pub(crate) fn standing(run: &Value) -> Standing {
    let lp = cur(run);
    let mut order = crate::val::strings(lp.at("rank").at("order"));
    let reviews = lp.list("elder_reviews");
    let reviewed = crate::val::strings(lp.at("reviewed"));
    let kills: Vec<String> = reviewed
        .iter()
        .filter(|c| {
            !reviews.is_empty()
                && 2 * reviews
                    .iter()
                    .filter(|r| r.at("verdict").s(c) == "kill")
                    .count()
                    > reviews.len()
        })
        .cloned()
        .collect();
    let prefs = |c: &str| reviews.iter().filter(|r| r.s("preferred") == c).count();
    let mut swapped = false;
    let live: Vec<String> = order
        .iter()
        .filter(|c| reviewed.contains(c) && !kills.contains(c))
        .cloned()
        .collect(); // the reviewed cards, in vote order
    if live.len() >= 2 {
        // the elders' clear favourite leads
        let mut best = &live[0];
        for c in &live[1..] {
            if prefs(c) > prefs(best) {
                best = c;
            }
        }
        if prefs(best) > prefs(&live[0]) {
            order.retain(|c| c != best);
            let at = order.iter().position(|c| *c == live[0]).unwrap_or(0);
            order.insert(at, best.clone());
            swapped = true;
        }
    }
    let mut ranked: Vec<String> = order
        .iter()
        .filter(|c| !kills.contains(c))
        .cloned()
        .collect();
    ranked.extend(kills.iter().cloned());
    let leader = ranked.first().cloned().unwrap_or_default();
    let s1: Vec<Value> = reviews
        .iter()
        .flat_map(|rv| rv.list("top_risks").iter())
        .filter(|r| r.s("severity") == "S1")
        .cloned()
        .collect();
    let ship = !reviews.is_empty()
        && reviews
            .iter()
            .all(|rv| rv.at("verdict").s(&leader) == "ship" && rv.s("preferred") == leader);
    let voices: HashSet<&str> = reviews.iter().map(|rv| rv.s("preferred")).collect();
    Standing {
        unreviewed_leader: !reviews.is_empty() && !reviewed.contains(&leader),
        order: ranked,
        kills,
        swapped,
        control_votes: prefs("CONTROL"),
        elders: reviews.len(),
        s1,
        all_ship_leader: ship,
        split: voices.len() > 1,
        leader,
    }
}

/// Why this cut needs the requester (or an explicit unattended choice) and not just the count.
pub(crate) fn judgment_points(st: &Standing, last: bool) -> Vec<String> {
    let mut why = Vec::new();
    if st.split {
        why.push("the elders split".to_string());
    }
    if !st.s1.is_empty() {
        why.push("an elder raised an S1".to_string());
    }
    if last && st.unreviewed_leader {
        why.push(format!(
            "the elders never reviewed the leader {} (they killed what they did review)",
            st.leader
        ));
    }
    why
}

/// One line on a vote: how many ballots, how much they agreed, and what the vote could not judge.
pub(crate) fn vote_line(v: &Value) -> String {
    let mut line = format!(
        "{} blind ballots, {} head-to-heads counted, {} sat out (the voter alone wrote one side); agreement {} ({})",
        v.show("ballots"),
        v.show("head_to_heads"),
        v.show("recused"),
        v.show("agreement"),
        v.show("agreement_note")
    );
    if v.truthy("gaps") {
        line.push_str(&format!(
            "; {} gap(s) on ballots counted as no opinion",
            v.show("gaps")
        ));
    }
    if v.truthy("thin") {
        let pairs: Vec<String> = v
            .list("thin")
            .iter()
            .map(|p| crate::val::strings(p).join(" vs "))
            .collect();
        line.push_str(&format!(
            "; LESS THAN ONE FULL VOTE for {} (the vote alone may not cut either)",
            pairs.join(", ")
        ));
    }
    line
}

/// The titles of a loop's candidates, by id.
pub(crate) fn titles_of(lp: &Value) -> HashMap<String, String> {
    lp.list("candidates")
        .iter()
        .map(|c| (c.show("id"), c.show("title")))
        .collect()
}

fn next_cut(run: &Value) -> Result<Vec<String>> {
    let (lp, st) = (cur(run), standing(run));
    let titles = titles_of(lp);
    let last = is_last(run);
    let keep = if last {
        "1".to_string()
    } else {
        run.list("schedule")
            .get(loop_no(run))
            .map(py::to_str)
            .unwrap_or_else(|| "1".to_string())
    };
    let mut out = vec!["Standing (rank vote, checks, elder verdicts):".to_string()];
    for (i, c) in st.order.iter().enumerate() {
        let killed = if st.kills.contains(c) {
            "  [killed by elders]"
        } else {
            ""
        };
        out.push(format!(
            "  {}. {c}: {}{killed}",
            i + 1,
            titles.get(c).cloned().unwrap_or_default()
        ));
    }
    out.push(format!("Rank vote: {}", vote_line(lp.at("rank"))));
    if st.elders > 0 {
        out.push(format!(
            "Elders: {} of {} prefer a council candidate; {} prefer the control arm.",
            st.elders - st.control_votes,
            st.elders,
            st.control_votes
        ));
    }
    if st.swapped {
        out.push("Elders preferred the runner-up: it now leads.".to_string());
    }
    let why = judgment_points(&st, last);
    if !why.is_empty() {
        out.push(format!("JUDGMENT POINT: {}. Ask the requester ONE targeted question and pass it with --asked \"Q -> A\".", why.join("; ")));
        out.push(
            "  If they cannot be asked: take the safer option (nothing irreversible), pass it with \
             --unattended \"<option>\", and flag it at the top of the write-up."
                .to_string(),
        );
        for x in &st.s1 {
            let candidate = x
                .get("candidate")
                .map(py::to_str)
                .unwrap_or_else(|| "?".to_string());
            out.push(format!(
                "  S1 ({candidate}): {}",
                py::head(&x.show("risk"), 260)
            ));
        }
    }
    if !last && st.all_ship_leader && st.s1.is_empty() {
        out.push("Every elder prefers the leader and says ship, with no S1: you may stop early (--stop \"converged\").".to_string());
    }
    out.push(format!("This cut keeps {keep}."));
    out.push(
        "THEN  council cut --eli5 \"<what happened this loop, one line>\" [--changed \"...\"] [--asked \"Q -> A\" | \
         --unattended \"<option>\"] [--stop \"reason\"]"
            .to_string(),
    );
    Ok(out)
}

/// `run.get(key, fallback)` as text.
fn shown_or(run: &Value, key: &str, fallback: &str) -> String {
    run.get(key)
        .map(py::to_str)
        .unwrap_or_else(|| fallback.to_string())
}

fn next_final(run: &Value, d: &Path) -> Result<Vec<String>> {
    let f = d.join("final.md");
    let Some(last) = run.list("loops").last() else {
        return fail("this run has no loop to write up");
    };
    let Some(win) = last
        .list("candidates")
        .iter()
        .find(|c| c.at("id") == run.at("winner"))
    else {
        return fail(format!(
            "the winner {} is not among the last loop's candidates",
            run.show("winner")
        ));
    };
    let control_line = shown_or(run, "control_line", "not run");
    if !f.exists() {
        let mut dropped: Vec<String> = Vec::new();
        for (k, lp) in run.list("loops").iter().enumerate() {
            let gone = lp.list("eliminated");
            dropped.extend(
                lp.list("candidates")
                    .iter()
                    .filter(|c| gone.contains(c.at("id")))
                    .map(|c| {
                        format!(
                            "| {} | cut in loop {}: TODO(chair) one-line reason |",
                            c.show("title"),
                            k + 1
                        )
                    }),
            );
        }
        let mut risks: Vec<String> = last
            .list("red_team")
            .iter()
            .filter(|x| matches!(x.s("severity"), "S1" | "S2"))
            .map(|x| {
                let note = x.at("response").at("note");
                let note = if py::truthy(note) {
                    py::to_str(note)
                } else {
                    "TODO(chair)".to_string()
                };
                format!(
                    "| {} | {} | {note} | TODO(chair) |",
                    x.show("finding"),
                    x.show("severity")
                )
            })
            .collect();
        for rv in last.list("elder_reviews") {
            risks.extend(rv.list("top_risks").iter().take(3).map(|r| {
                format!(
                    "| {} | {} | TODO(chair) | TODO(chair) |",
                    r.show("risk"),
                    r.show("severity")
                )
            }));
        }
        let dissent: Vec<String> = last
            .list("elder_reviews")
            .iter()
            .filter(|rv| rv.at("preferred") != run.at("winner"))
            .map(|rv| {
                let preferred = if rv.s("preferred") == "CONTROL" {
                    "the control arm".to_string()
                } else {
                    rv.show("preferred")
                };
                format!(
                    "- {} ({}): preferred {preferred}; asked for: {}",
                    rv.show("seat"),
                    rv.show("model"),
                    shown_or_empty(rv, "key_fix")
                )
            })
            .collect();
        let or = |rows: Vec<String>, none: &str| {
            if rows.is_empty() {
                none.to_string()
            } else {
                rows.join("\n")
            }
        };
        let deviations = crate::val::strings(run.at("deviations"));
        let vars = crate::vars!(
            dropped_rows = or(dropped, "| (none) | |"),
            risk_rows = or(risks, "| (none recorded) | | | |"),
            dissent = or(dissent, "- None: every elder preferred the winner."),
            gate = run.show("gate"),
            winner_title = win.show("title"),
            control_line = control_line,
            seat_line = seat_line(run),
            deviations = if deviations.is_empty() {
                "none".to_string()
            } else {
                deviations.join("; ")
            }
        );
        core::write_text(&f, &tpl("final-writeup.md", &vars)?)?;
    }
    let mut out = vec![
        format!("Winner: {}", win.show("title")),
        format!("Control arm: {control_line}"),
    ];
    let control_file = show(&d.join("control").join("CTRL.out.json"));
    if run.s("control_result") == "lost" {
        out.push(format!(
            "THE ONE-SHOT BASELINE WON. Say so plainly in the write-up. Adopt it, or merge its strengths: read {control_file}"
        ));
    } else if run.at("control").truthy("done") {
        out.push(format!(
            "You may now read the control answer and graft anything it did better: {control_file}"
        ));
    }
    let sh = run.at("shared");
    if sh.truthy("facts_added") || sh.truthy("facts_merged") {
        out.push(format!(
            "Evidence: this run filed {} fact(s) in the shared memory ({}/research). Cite the source of every fact you use.",
            shown_or(sh, "facts_added", "0"),
            run.at("memory").show("dir")
        ));
    }
    let still = open_requests(run).len();
    if still > 0 {
        out.push(format!("{still} question(s) for the researchers were never answered: list them as open questions."));
    }
    out.push(format!(
        "Write the final for humans: {}  (replace every TODO(chair))",
        show(&f)
    ));
    out.push(
        "  Plain words, answer first, one diagram, evidence, risks, dissent, falsifiable metrics."
            .to_string(),
    );
    out.push("  Merge in the best compatible ideas from the runners-up and the elders. Label every stand-in seat.".to_string());
    out.push("THEN  council ingest".to_string());
    Ok(out)
}

fn next_retro(run: &Value, d: &Path) -> Result<Vec<String>> {
    let f = d.join("retro.md");
    if !f.exists() {
        let calls: Vec<String> = items(run.at("calls"))
            .map(|(k, v)| format!("{k} {}", py::to_str(v)))
            .collect();
        let agree: Vec<String> = run
            .list("loops")
            .iter()
            .map(|lp| lp.at("rank").show("agreement"))
            .collect();
        let deviations = crate::val::strings(run.at("deviations"));
        let mut md = vec![
            format!("# Retro: {}", run.s("id")),
            String::new(),
            "ELI5: TODO(chair) one line".to_string(),
            String::new(),
            "- What worked: TODO(chair)".to_string(),
            "- What did not: TODO(chair)".to_string(),
            format!(
                "- Calls: {}. Total {} (the gate estimated {}).",
                calls.join(", "),
                calls_so_far(run),
                run.show("calls_estimate")
            ),
            format!(
                "- Loops used: {} of {}. Rank-vote agreement per loop: {}.",
                run.list("loops").len(),
                run.show("max_loops"),
                agree.join(", ")
            ),
            format!(
                "- Control arm: {}",
                shown_or(run, "control_line", "not run")
            ),
            format!(
                "- Deviations: {}",
                if deviations.is_empty() {
                    "none".to_string()
                } else {
                    deviations.join("; ")
                }
            ),
        ];
        if run.truthy("memory") {
            let (sh, posts, mem) = (run.at("shared"), run.list("commons"), run.at("memory"));
            md.push(format!(
                "- Shared memory ({}: {}): {} fact(s) added, {} already held, {} refused; {} note(s), {} of them questions for the researchers ({} never answered).",
                mem.show("backend"),
                mem.show("dir"),
                shown_or(sh, "facts_added", "0"),
                shown_or(sh, "facts_merged", "0"),
                shown_or(sh, "facts_refused", "0"),
                posts.len(),
                posts.iter().filter(|x| x.s("to") == "research").count(),
                open_requests(run).len()
            ));
            let m = run.at("map");
            md.push(format!(
                "- Codebase map: {}",
                if py::truthy(m) {
                    format!("{} ({})", m.show("path"), m.show("reason"))
                } else {
                    "no mapper in this run".to_string()
                }
            ));
        }
        md.push("- Friction reported by the seats:".to_string());
        let friction = run.list("friction");
        if friction.is_empty() {
            md.push("  - none".to_string());
        } else {
            md.extend(friction.iter().map(|x| format!("  - {}", py::to_str(x))));
        }
        md.push(String::new());
        md.push(
            "## Lessons (0 to 3, one line each; they reach docs/lessons.md only through a PR)"
                .to_string(),
        );
        md.push(format!(
            "<!-- - [L-{}-1] (helpful: 0 | harmful: 0) The lesson. Evidence: {} -->",
            crate::clock::today().format("%Y%m%d"),
            run.s("id")
        ));
        let used = run.at("brief").list("lessons_applied");
        if !used.is_empty() {
            md.push(String::new());
            md.push(
                "## Lessons used in this run (bump helpful or harmful in the same PR)".to_string(),
            );
            md.extend(used.iter().map(|x| {
                format!(
                    "- {}: TODO(chair) helpful or harmful, and why",
                    py::to_str(x)
                )
            }));
        }
        core::write_text(&f, &format!("{}\n", md.join("\n")))?;
    }
    Ok(vec![
        format!("Write the retro: {}  (replace every TODO(chair); add 0 to 3 lesson lines)", show(&f)),
        "  Lessons must be things a future run can act on. Seat friction notes are prompts to fix the packets.".to_string(),
        "THEN  council ingest".to_string(),
    ])
}

fn next_done(d: &Path) -> Vec<String> {
    vec![
        format!("Run complete. Final: {}", show(&d.join("final.md"))),
        "Open a PR that appends the proposed lessons to docs/lessons.md and adds a row to docs/outcomes.md \
         (success metric, kill criterion, control-arm result). Check the outcome again at 30 and 90 days."
            .to_string(),
    ]
}
