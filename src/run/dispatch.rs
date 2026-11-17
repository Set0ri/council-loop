//! `council dispatch`: run the seats of this step that sit behind a command-line tool or an API.

use super::*;

/// What a seat may do while it works. A researcher gets the web, or the project's files when its
/// beat is the project's own record: never both in one seat, so a web page cannot talk it into
/// sending a file out. The mapper and the seats that propose or attack read files. Judges read
/// only their packet.
pub(crate) fn tools_for(seat: &Value, phase: &str) -> &'static str {
    match seat.s("role") {
        "researcher" if seat.s("beat") == core::BEATS[3].0 => "read",
        "researcher" => "web",
        "mapper" => "read",
        _ if matches!(phase, "ideate" | "redteam") => "read",
        _ => "none",
    }
}

/// The command of a dry run, on one line. (A command given as one string is spelled out letter by
/// letter, as the reference does when it joins it.)
fn command_line(res: &Value) -> String {
    let joined = match res.at("command") {
        Value::Array(parts) => parts.iter().map(py::to_str).collect::<Vec<_>>().join(" "),
        Value::String(text) => text.chars().map(String::from).collect::<Vec<_>>().join(" "),
        _ => String::new(),
    };
    py::head(&joined, 160).to_string()
}

/// One seat to send: its packet, and where the answer goes.
struct Job {
    seat: String,
    how: Value,
    prompt: String,
    out: PathBuf,
}

/// `council dispatch`: send every CLI or API seat of this step its prompt and save what comes back.
pub fn cmd_dispatch(run_id: Option<&str>, seat: Option<&str>, dry_run: bool) -> Result<i32> {
    let (run, d) = load(run_id)?;
    let only = seat.filter(|s| !s.is_empty());
    let phase = run.s("phase").to_string();
    // Only pure ballot steps (vote/rank) may dispatch subagent/self seats over tool-less HTTP.
    // Elders, ideation, red-team, researchers, mapper and control seats keep their full tools.
    let ballot_only = matches!(phase.as_str(), "vote" | "rank");
    let has_openrouter = ballot_only && providers::has_openrouter_key();
    let (mut todo, mut capped): (Vec<Job>, Vec<String>) = (Vec::new(), Vec::new());
    for f in seat_files(&run, &d)? {
        let v = run.at("seats").at(&f.seat);
        let via = v.s("via");
        if only.is_some_and(|s| s != f.seat) {
            continue;
        }
        let how = if via.starts_with("cli:") || via == "openrouter" {
            v.clone()
        } else if has_openrouter && matches!(via, "subagent" | "self") {
            let model_hint = if v.truthy("stand_in_for") {
                v.s("stand_in_for")
            } else {
                v.s("model")
            };
            let Some(slug) = providers::openrouter_slug(
                Some(model_hint),
                v.at("creator").as_str(),
                v.at("native_id").as_str(),
            ) else {
                continue;
            };
            let mut routed = v.clone();
            routed["provider"] = json!("openrouter");
            routed["native_id"] = json!(slug);
            routed["via"] = json!("openrouter");
            routed
        } else {
            continue;
        };
        if f.out.exists() && only.is_none() {
            continue;
        }
        let rejected = run
            .at("attempts")
            .int(&attempt_key(&run, &f.seat))
            .unwrap_or(0);
        if rejected >= 2 && only.is_none() {
            capped.push(f.seat); // two rejected outputs: stop paying for more
            continue;
        }
        let retry = retry_file(&f.prompt);
        let prompt = crate::safe::read_capped(
            if retry.exists() { &retry } else { &f.prompt },
            crate::safe::MAX_OUTPUT * 4,
        )?;
        todo.push(Job {
            seat: f.seat,
            how,
            prompt,
            out: f.out,
        });
    }
    for s in &capped {
        say!(
            "  {s}: rejected twice, not sent again. Drop it (council ingest --drop {s} --reason \"<why>\") or insist (council dispatch --seat {s})."
        );
    }
    if todo.is_empty() {
        say!("nothing to dispatch: no CLI/API seat of this step is missing an output");
        return Ok(0);
    }
    let timeout = if ballot_only { 120 } else { 900 };
    let timed: Vec<(Value, u64)> = providers::in_parallel(&todo, 8, |job| {
        let v = &job.how;
        let (model, effort) = (v.at("native_id").as_str(), v.at("effort").as_str());
        let started = std::time::Instant::now();
        let mut res = providers::ask(
            v.s("provider"),
            &job.prompt,
            model,
            effort,
            timeout,
            dry_run,
            tools_for(v, &phase),
        );
        if res.truthy("ok") && !dry_run {
            let is_json_out = job
                .out
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json"));
            let to_save = if v.s("provider") == "openrouter" && is_json_out {
                match providers::parse_json(res.s("text")) {
                    Some(parsed) => Some(Json::ONE.dumps(&parsed)),
                    None => {
                        res["ok"] = json!(false);
                        res["error"] = json!(format!(
                            "the reply did not contain a JSON object: {}",
                            py::head(&py::squash(res.s("text")), 200)
                        ));
                        None
                    }
                }
            } else {
                Some(res.s("text").to_string())
            };
            if let Some(payload) = to_save {
                if let Err(err) = core::write_text(&job.out, &payload) {
                    res["ok"] = json!(false);
                    res["error"] = json!(format!("the answer could not be saved: {err:#}"));
                }
            }
        }
        if res.truthy("error") {
            // what a vendor's tool prints on failure can hold a key: it goes no further
            res["error"] = json!(crate::log::redact(&res.show("error")));
        }
        let ms = crate::log::took(started);
        if !dry_run {
            crate::log::progress(&format!(
                "seat {}: {}",
                job.seat,
                if res.truthy("ok") {
                    "answered"
                } else {
                    "failed"
                }
            ));
        }
        (res, ms)
    });
    if !dry_run {
        // the diary is written here, in seat order, not from the worker threads
        for (job, (res, ms)) in todo.iter().zip(&timed) {
            crate::log::record(
                &d,
                json!({"event": "seat_call", "seat": job.seat, "step": phase, "provider": job.how.at("provider"),
                       "model": res.at("model"), "via": job.how.at("via"), "tools": tools_for(&job.how, &phase),
                       "ok": res.truthy("ok"), "ms": ms, "prompt_chars": py::len(&job.prompt),
                       "reply_chars": py::len(res.s("text")),
                       "error": if res.truthy("ok") { Value::Null } else { json!(py::head(py::strip(&res.show("error")), 300)) }}),
            );
        }
    }
    let results: Vec<Value> = timed.into_iter().map(|(res, _)| res).collect();
    let mut failed = 0;
    for (s, res) in todo.iter().map(|job| &job.seat).zip(&results) {
        if dry_run {
            say!("  {s}: would run {}", command_line(res));
        } else if res.truthy("ok") {
            say!("  {s}: ok ({})", res.show("model"));
        } else {
            failed += 1;
            say!(
                "  {s}: FAILED: {}",
                py::head(py::strip(&res.show("error")), 300)
            );
        }
    }
    if failed > 0 {
        say!("A seat that cannot be reached can be left out: council ingest --drop <seat> --reason \"<why>\"");
    }
    say!("THEN  council ingest");
    Ok(i32::from(failed > 0))
}
