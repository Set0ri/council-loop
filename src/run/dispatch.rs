//! `council dispatch`: run the seats of this step that sit behind a command-line tool or an API.

use super::*;

/// What a seat may do while it works: researchers read files and open web pages, the mapper and
/// the seats that propose or attack read files, judges read only their packet.
fn tools_for(seat: &Value, phase: &str) -> &'static str {
    match seat.s("role") {
        "researcher" => "research",
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
    let (mut todo, mut capped): (Vec<Job>, Vec<String>) = (Vec::new(), Vec::new());
    for f in seat_files(&run, &d)? {
        let v = run.at("seats").at(&f.seat);
        let via = v.s("via");
        if only.is_some_and(|s| s != f.seat) || !(via.starts_with("cli:") || via == "openrouter") {
            continue;
        }
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
        let prompt = core::read_text(if retry.exists() { &retry } else { &f.prompt })?;
        todo.push(Job {
            seat: f.seat,
            how: v.clone(),
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
    let results: Vec<Value> = providers::in_parallel(&todo, 8, |job| {
        let v = &job.how;
        let (model, effort) = (v.at("native_id").as_str(), v.at("effort").as_str());
        let mut res = providers::ask(
            v.s("provider"),
            &job.prompt,
            model,
            effort,
            900,
            dry_run,
            tools_for(v, &phase),
        );
        if res.truthy("ok") && !dry_run {
            if let Err(err) = core::write_text(&job.out, res.s("text")) {
                res["ok"] = json!(false);
                res["error"] = json!(format!("the answer could not be saved: {err:#}"));
            }
        }
        res
    });
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
