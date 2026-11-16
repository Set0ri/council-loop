//! Whole scenarios, end to end, through the real binary.
//!
//! Two kinds of test live here, and one stand-in.
//!
//! **Recorded scenarios** (`tests/cassettes/*.json`). Each is a story: files a person, a chair or a
//! seat wrote, commands that were run in between, and what every command printed and left on disk.
//! The story is replayed step by step in fresh folders with the clock frozen, and every exit code,
//! every printed line and every file at the end must be the same as recorded. The recordings were
//! made when the Rust build had been shown, command for command, to behave like the Python
//! reference it replaced (see docs/LEGACY.md), so they carry that proof forward.
//!
//! When a change alters what the tool prints on purpose, accept the new outputs and review the
//! diff like any other change:
//!
//! ```text
//! COUNCIL_ACCEPT=1 cargo test --test scenarios
//! ```
//!
//! Only what the tool printed and wrote is re-recorded; what the world wrote stays as it is.
//!
//! **Played scenarios** use the scripted council that also drives `council demo`: a whole Lite run,
//! a whole Full run, and a run whose every seat sits behind a command-line tool.
//!
//! **The stand-in.** Those command-line tools (`claude`, `codex`, `gemini`) are this test program
//! itself under another name: started with `COUNCIL_TEST_TOOL` set, it reads a packet on standard
//! input and answers as the script would, in the output convention of the tool it is named after.
//! Our adapters run for real; no model does.
//!
//! This file has its own `main` (the stand-in needs it). Run one scenario by name:
//! `cargo test --test scenarios -- lite`.

use council_loop::demo::{self, script, Stage};
use serde_json::{json, Map, Value};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::Command;

fn council() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_council"))
}

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A fresh folder under the system's temporary folder, spelled the way the file system spells it.
fn fresh(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("council-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the temporary folder is writable");
    dunce::canonicalize(&dir).expect("the folder was just made")
}

fn copy_tree(from: &Path, to: &Path, skip: &[&str]) {
    std::fs::create_dir_all(to).expect("folder");
    for entry in std::fs::read_dir(from).expect("readable").flatten() {
        let name = entry.file_name();
        if skip.iter().any(|s| name.to_string_lossy() == *s) {
            continue;
        }
        let (src, dst) = (entry.path(), to.join(&name));
        if src.is_dir() {
            copy_tree(&src, &dst, skip);
        } else {
            std::fs::copy(&src, &dst).expect("copy");
        }
    }
}

// ----------------------------------------------------------------------------- recorded scenarios

/// The folders of one replay, and how their names are swapped for placeholders and back.
struct World {
    base: PathBuf,
    /// The address of the stand-in for the model-data API, when the scenario has one.
    aa: Option<String>,
}

impl World {
    fn pairs(&self) -> Vec<(String, &'static str)> {
        let text = |p: PathBuf| p.display().to_string();
        vec![
            (text(self.base.join("proj")), "<ROOT>"),
            (text(self.base.join("home")), "<HOME>"),
            (text(self.base.clone()), "<BASE>"),
            (text(repo().join("skills").join("council-loop")), "<SKILL>"),
            (text(repo()), "<REPO>"),
        ]
    }

    /// Placeholders -> this replay's folders.
    fn fill(&self, text: &str) -> String {
        let mut out = text.to_string();
        for (path, name) in self.pairs() {
            out = out.replace(name, &path);
        }
        if let Some(aa) = &self.aa {
            out = out.replace("<AA>", aa);
        }
        out
    }

    /// This replay's folders -> placeholders, and the one line that names the build left out.
    fn norm(&self, text: &str) -> String {
        let mut out = text.to_string();
        for (path, name) in self.pairs() {
            out = out.replace(&path, name);
        }
        if let Some(aa) = &self.aa {
            out = out.replace(aa.as_str(), "<AA>");
        }
        out.split_inclusive('\n')
            .filter(|line| {
                let line = line.trim_start();
                !(line.starts_with("\"council\": \"") || line.starts_with("\"python\": \""))
            })
            .collect()
    }

    fn run(&self, step: &Value) -> (i64, String, String) {
        let args: Vec<String> = step["cmd"]
            .as_array()
            .expect("cmd is a list")
            .iter()
            .map(|a| self.fill(a.as_str().expect("an argument is text")))
            .collect();
        let clock = step["clock"].as_u64().unwrap_or(0);
        let mut cmd = Command::new(council());
        cmd.args(&args)
            .current_dir(match step["cwd"].as_str() {
                Some(cwd) => PathBuf::from(self.fill(cwd)),
                None => self.base.join("proj"),
            })
            .stdin(std::process::Stdio::null());
        for (name, _) in std::env::vars_os() {
            let name = name.to_string_lossy().into_owned();
            if name.starts_with("COUNCIL_")
                || matches!(
                    name.as_str(),
                    "CLAUDECODE" | "OPENROUTER_API_KEY" | "AA_API_KEY" | "CI"
                )
            {
                cmd.env_remove(name);
            }
        }
        cmd.env("COUNCIL_HOME", self.base.join("home").join(".council"))
            .env("CLAUDECODE", "1")
            .env(
                "COUNCIL_NOW",
                format!("2026-11-13T12:{:02}:{:02}", clock / 60, clock % 60),
            )
            .env(
                "COUNCIL_SKILL_DIR",
                repo().join("skills").join("council-loop"),
            )
            .env("COUNCIL_PRIVATE_DIR", self.base.join("private"))
            .env("COUNCIL_PARSERS", "built-in")
            .env("COUNCIL_NO_WELCOME", "1");
        for (name, value) in step["env"].as_object().into_iter().flatten() {
            match value.as_str() {
                Some(value) => cmd.env(name, self.fill(value)),
                None => cmd.env_remove(name),
            };
        }
        let done = cmd.output().expect("the binary runs");
        (
            i64::from(done.status.code().unwrap_or(-1)),
            self.norm(&String::from_utf8_lossy(&done.stdout)),
            self.norm(&String::from_utf8_lossy(&done.stderr)),
        )
    }

    /// Every file of the world at the end: its placeholder name, and its text (or, for a JSON file,
    /// its value: the same meaning, not just the same bytes).
    fn snapshot(&self) -> Map<String, Value> {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .collect();
            entries.sort();
            for path in entries {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if name == ".git" || name == "__pycache__" {
                    continue;
                }
                if path.is_dir() {
                    walk(&path, out);
                } else {
                    out.push(path);
                }
            }
        }
        let mut files = Vec::new();
        for top in ["proj", "home", "private", "vault", "skill/models"] {
            walk(&self.base.join(top), &mut files);
        }
        let mut out = Map::new();
        for file in files {
            let name = self.norm(&file.display().to_string());
            let entry = match std::fs::read(&file).map(String::from_utf8) {
                Ok(Ok(text)) => {
                    let text = self.norm(&text);
                    match serde_json::from_str::<Value>(&text) {
                        Ok(value) if name.ends_with(".json") => json!({"json": value}),
                        _ => json!({"text": text}),
                    }
                }
                _ => json!({"text": "<not text>"}),
            };
            out.insert(name, entry);
        }
        out
    }
}

/// A tiny stand-in for the Artificial Analysis API: one path, one key, one recorded body.
fn serve_model_data(http: &Value) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a free port");
    let address = format!("http://{}", listener.local_addr().expect("bound"));
    let (path, key) = (
        http["path"].as_str().unwrap_or("").to_string(),
        http["key"].as_str().unwrap_or("").to_string(),
    );
    let (body, denied) = (
        http["body"].as_str().unwrap_or("").to_string(),
        http["denied"].as_str().unwrap_or("").to_string(),
    );
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let mut head = Vec::new();
            let mut chunk = [0u8; 2048];
            while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                match stream.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => head.extend_from_slice(&chunk[..n]),
                }
            }
            let head = String::from_utf8_lossy(&head).to_lowercase();
            let asked = head.split_whitespace().nth(1).unwrap_or("").to_string();
            let keyed = head
                .lines()
                .any(|l| l.trim() == format!("x-api-key: {key}"));
            let (status, text) = match (asked == path, keyed) {
                (true, true) => ("200 OK", &body),
                (true, false) => ("401 Unauthorized", &denied),
                _ => ("404 Not Found", &denied),
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                text.len()
            );
        }
    });
    address
}

fn first_difference(want: &str, got: &str) -> String {
    let (want_lines, got_lines): (Vec<&str>, Vec<&str>) =
        (want.lines().collect(), got.lines().collect());
    let at = want_lines
        .iter()
        .zip(&got_lines)
        .position(|(a, b)| a != b)
        .unwrap_or(want_lines.len().min(got_lines.len()));
    let show = |lines: &[&str]| {
        lines
            .iter()
            .skip(at.saturating_sub(1))
            .take(4)
            .map(|l| format!("      {}", l.chars().take(300).collect::<String>()))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "    line {}:\n    recorded:\n{}\n    now:\n{}",
        at + 1,
        show(&want_lines),
        show(&got_lines)
    )
}

/// Replay one cassette. `Ok(note)` when everything matched (or was accepted), `Err(what differs)`.
fn replay(file: &Path) -> Result<String, String> {
    let accept = std::env::var("COUNCIL_ACCEPT").is_ok_and(|v| v == "1");
    let mut cassette: Value =
        serde_json::from_str(&std::fs::read_to_string(file).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let name = cassette["name"].as_str().unwrap_or("?").to_string();
    let needs_git = cassette["needs"]
        .as_array()
        .is_some_and(|n| n.iter().any(|x| x == "git"));
    if needs_git && Command::new("git").arg("--version").output().is_err() {
        return Ok("skipped: it needs git".to_string());
    }
    let base = fresh(&name);
    std::fs::create_dir_all(base.join("proj")).map_err(|e| e.to_string())?;
    let world = World {
        aa: cassette.get("http").map(serve_model_data),
        base,
    };
    let mut problems: Vec<String> = Vec::new();
    let mut changed = false;
    let steps = cassette["steps"].as_array().cloned().unwrap_or_default();
    for (i, step) in steps.iter().enumerate() {
        if let Some(target) = step["put"].as_str() {
            let path = PathBuf::from(world.fill(target));
            std::fs::create_dir_all(path.parent().expect("a file has a folder"))
                .map_err(|e| e.to_string())?;
            std::fs::write(&path, world.fill(step["text"].as_str().unwrap_or("")))
                .map_err(|e| e.to_string())?;
        } else if let Some(target) = step["rm"].as_str() {
            let path = PathBuf::from(world.fill(target.trim_end_matches('/')));
            let _ = if path.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
        } else if let Some(target) = step["mkdir"].as_str() {
            std::fs::create_dir_all(world.fill(target)).map_err(|e| e.to_string())?;
        } else if let Some(target) = step["git_init"].as_str() {
            Command::new("git")
                .args(["init", "-q"])
                .arg(world.fill(target))
                .output()
                .map_err(|e| e.to_string())?;
        } else if let Some(target) = step["copy_skill"].as_str() {
            copy_tree(
                &repo().join("skills").join("council-loop"),
                Path::new(&world.fill(target)),
                &["scripts", "__pycache__"],
            );
        } else if step.get("cmd").is_some() {
            let (rc, out, err) = world.run(step);
            let loose = step["loose"].as_bool().unwrap_or(false);
            let shown = step["cmd"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();
            let mut differs = Vec::new();
            if step["rc"].as_i64() != Some(rc) {
                differs.push(format!(
                    "    exit code: recorded {}, now {rc}\n    stderr now: {}",
                    step["rc"],
                    err.chars().take(400).collect::<String>()
                ));
            }
            if step["out"].as_str() != Some(out.as_str()) {
                differs.push(format!(
                    "  standard output:\n{}",
                    first_difference(step["out"].as_str().unwrap_or(""), &out)
                ));
            }
            if !loose && step["err"].as_str() != Some(err.as_str()) {
                differs.push(format!(
                    "  standard error:\n{}",
                    first_difference(step["err"].as_str().unwrap_or(""), &err)
                ));
            }
            if !differs.is_empty() {
                if accept {
                    changed = true;
                    let recorded = &mut cassette["steps"][i];
                    recorded["rc"] = json!(rc);
                    recorded["out"] = json!(out);
                    recorded["err"] = json!(if loose { String::new() } else { err });
                } else {
                    problems.push(format!(
                        "  step {} `council {}`:\n{}",
                        i + 1,
                        shown.chars().take(160).collect::<String>(),
                        differs.join("\n")
                    ));
                    if problems.len() >= 3 {
                        break; // later steps build on these: their differences would only repeat the first
                    }
                }
            }
        }
    }
    if problems.is_empty() {
        let now = world.snapshot();
        let puts: Map<String, Value> = steps
            .iter()
            .filter_map(|s| Some((s["put"].as_str()?.to_string(), s["text"].clone())))
            .collect();
        let mut recorded = cassette["final"].as_object().cloned().unwrap_or_default();
        // a file the world wrote and nobody changed is recorded by name only: its content is in the steps
        for (name, entry) in recorded.iter_mut() {
            if entry["as_put"] == true {
                let text = puts
                    .get(name)
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                *entry = match serde_json::from_str::<Value>(&text) {
                    Ok(value) if name.ends_with(".json") => json!({"json": value}),
                    _ => json!({"text": text}),
                };
            }
        }
        let mut names: Vec<&String> = recorded.keys().chain(now.keys()).collect();
        names.sort();
        names.dedup();
        for name in names {
            match (recorded.get(name), now.get(name)) {
                (Some(a), Some(b)) if a == b => {}
                (Some(a), Some(b)) => problems.push(format!(
                    "  file {name} differs:\n{}",
                    first_difference(
                        &a["text"].as_str().map(str::to_string).unwrap_or_else(|| {
                            serde_json::to_string_pretty(&a["json"]).unwrap_or_default()
                        }),
                        &b["text"].as_str().map(str::to_string).unwrap_or_else(|| {
                            serde_json::to_string_pretty(&b["json"]).unwrap_or_default()
                        })
                    )
                )),
                (Some(_), None) => problems.push(format!("  file {name} was not written")),
                (None, Some(_)) => problems.push(format!("  file {name} is new")),
                (None, None) => {}
            }
        }
        if !problems.is_empty() && accept {
            changed = true;
            problems.clear();
            let mut fresh_final = Map::new();
            for (name, entry) in now {
                let as_put = puts.get(&name).and_then(Value::as_str).is_some_and(|text| {
                    entry["text"].as_str() == Some(text)
                        || serde_json::from_str::<Value>(text)
                            .is_ok_and(|v| name.ends_with(".json") && entry["json"] == v)
                });
                fresh_final.insert(
                    name,
                    if as_put {
                        json!({"as_put": true})
                    } else {
                        entry
                    },
                );
            }
            cassette["final"] = Value::Object(fresh_final);
        }
    }
    if changed {
        let text = serde_json::to_string_pretty(&cassette).map_err(|e| e.to_string())?;
        // one space of indent, as the recorder wrote it, so an accepted change is a small diff
        let text = text
            .lines()
            .map(|line| {
                let indent = line.len() - line.trim_start().len();
                format!("{}{}", " ".repeat(indent / 2), line.trim_start())
            })
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(file, text + "\n").map_err(|e| e.to_string())?;
    }
    if problems.is_empty() {
        let _ = std::fs::remove_dir_all(&world.base);
        let commands = steps.iter().filter(|s| s.get("cmd").is_some()).count();
        Ok(format!(
            "{commands} commands{}",
            if changed {
                ", new outputs ACCEPTED: review the diff"
            } else {
                ""
            }
        ))
    } else {
        Err(format!(
            "{}\n  (the folders were kept: {})",
            problems.join("\n"),
            world.base.display()
        ))
    }
}

// ----------------------------------------------------------------------------- played scenarios

fn stage(name: &str) -> Stage {
    let base = fresh(name);
    let stage = Stage::new(&base.join("proj"), &base.join("own"), &council()).expect("a stage");
    demo::write_project(&stage.dir).expect("the made-up project");
    stage.with_env("CI", None)
}

/// Play a whole run and check what must hold at the end of any run.
fn played(
    stage: &Stage,
    init: &[&str],
    dispatch: bool,
    want_loops: usize,
) -> Result<String, String> {
    let mut commands = 0;
    demo::play_with(stage, init, dispatch, |step| {
        commands += usize::from(step.played.is_some())
    })
    .map_err(|e| format!("  {e:#}"))?;
    let run = stage.run().map_err(|e| e.to_string())?;
    let check = |ok: bool, what: &str| if ok { Ok(()) } else { Err(format!("  {what}")) };
    check(run["phase"] == "done", "the run did not reach `done`")?;
    check(
        run["loops"].as_array().map(Vec::len) == Some(want_loops),
        "the run took another number of loops",
    )?;
    check(
        demo::winner_title(&run) == script::CARDS[0].0,
        "the script's favourite did not win",
    )?;
    let audit = stage.council(&["audit"]).map_err(|e| e.to_string())?;
    check(
        audit.code == 0 && audit.out.contains("audit: ok"),
        "the audit did not pass",
    )?;
    let checked = stage.council(&["check"]).map_err(|e| e.to_string())?;
    check(
        checked.code == 0,
        &format!("the protocol check failed: {}", checked.out),
    )?;
    let d = stage.run_dir().map_err(|e| e.to_string())?;
    for file in ["final.md", "retro.md", "unblinded.json", "events.jsonl"] {
        check(d.join(file).is_file(), &format!("{file} is missing"))?;
    }
    let events = std::fs::read_to_string(d.join("events.jsonl")).map_err(|e| e.to_string())?;
    let verbs = events
        .lines()
        .filter(|l| l.contains("\"event\": \"verb\""))
        .count();
    check(
        verbs == commands,
        &format!("the diary holds {verbs} steps for {commands} commands"),
    )?;
    check(
        stage.dir.join("docs").join("ARCHITECTURE.md").is_file(),
        "the mapper's map was not installed",
    )?;
    if dispatch {
        let calls = events
            .lines()
            .filter(|l| l.contains("\"event\": \"seat_call\""))
            .count();
        check(calls >= 9, &format!("only {calls} seat calls in the diary"))?;
        check(!events.contains("\"ok\": false"), "a seat call failed")?;
    }
    Ok(format!("{commands} commands, {want_loops} loop(s)"))
}

fn lite_played() -> Result<String, String> {
    let stage = stage("played-lite");
    played(
        &stage,
        &["--tier", "lite", "--mode", "fanout:claude"],
        false,
        1,
    )
}

fn full_played() -> Result<String, String> {
    let stage = stage("played-full");
    played(
        &stage,
        &["--tier", "full", "--mode", "fanout:claude"],
        false,
        3,
    )
}

/// The names the stand-in tool is installed under.
const TOOLS: [&str; 3] = ["claude", "codex", "gemini"];

/// A folder holding `claude`, `codex` and `gemini`: each is this test program under that name.
fn tools_dir(base: &Path) -> PathBuf {
    let dir = base.join("tools");
    std::fs::create_dir_all(dir.join("state")).expect("folder");
    let me = std::env::current_exe().expect("this program has a file");
    for name in TOOLS {
        let target = dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
        std::fs::copy(&me, &target).expect("copy");
    }
    dir
}

fn cli_stage(name: &str) -> (Stage, PathBuf) {
    let base = fresh(name);
    let tools = tools_dir(&base);
    let path = std::env::join_paths(std::iter::once(tools.clone()).chain(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    )))
    .expect("a PATH");
    let stage = Stage::new(&base.join("proj"), &base.join("own"), &council())
        .expect("a stage")
        .with_env("PATH", Some(&path.to_string_lossy()))
        .with_env(
            "COUNCIL_TEST_TOOL",
            Some(&tools.join("state").to_string_lossy()),
        )
        .with_env("CLAUDECODE", None) // no subagents: every seat goes through a command-line tool
        .with_env("COUNCIL_SEATS_VIA", None)
        .with_env("CI", None);
    demo::write_project(&stage.dir).expect("the made-up project");
    (stage, tools)
}

/// A run in which every seat sits behind a command-line tool, across three labs.
fn cli_seats() -> Result<String, String> {
    let (stage, _) = cli_stage("played-cli");
    let doctor = stage.council(&["doctor"]).map_err(|e| e.to_string())?;
    if !doctor.out.contains("\"strongest_mode\": \"crosslab\"") {
        return Err(format!(
            "  the three stand-in tools were not found:\n{}",
            doctor.out
        ));
    }
    played(
        &stage,
        &["--tier", "lite", "--mode", "crosslab", "--research", "1"],
        true,
        1,
    )
}

/// A tool that is installed but signed out fails the way it says, and a seat that answers in prose
/// first is sent its packet again with the reason.
fn cli_trouble() -> Result<String, String> {
    let (stage, _) = cli_stage("played-cli-trouble");
    let stage = stage.with_env("COUNCIL_TEST_TOOL_DOWN", Some("gemini"));
    let probe = stage
        .council(&["doctor", "--probe"])
        .map_err(|e| e.to_string())?;
    let check = |ok: bool, what: String| if ok { Ok(()) } else { Err(format!("  {what}")) };
    check(
        probe.out.contains("\"claude\": \"ok\"") && probe.out.contains("\"codex\": \"ok\""),
        format!("the probe: {}", probe.out),
    )?;
    check(
        probe
            .out
            .contains("the gemini CLI is installed but did not answer a test prompt"),
        format!(
            "the probe did not report the signed-out tool: {}",
            probe.out
        ),
    )?;
    // a seat that answers in prose: rejected once, sent again with the reason, then accepted
    let stage = stage
        .with_env("COUNCIL_TEST_TOOL_DOWN", None)
        .with_env("COUNCIL_TEST_TOOL_PROSE", Some("L1"));
    let init = stage
        .council(&[
            "init",
            demo::ASK,
            "--tier",
            "lite",
            "--mode",
            "fanout:claude",
            "--no-research",
            "--map",
            "never",
        ])
        .map_err(|e| e.to_string())?;
    check(init.code == 0, format!("init: {}{}", init.out, init.err))?;
    std::fs::write(
        stage
            .run_dir()
            .map_err(|e| e.to_string())?
            .join("brief.yaml"),
        script::brief(&stage.run().map_err(|e| e.to_string())?),
    )
    .map_err(|e| e.to_string())?;
    let run = |args: &[&str]| stage.council(args).map_err(|e| e.to_string());
    check(
        run(&["ingest"])?.code == 0,
        "the brief was not accepted".to_string(),
    )?;
    check(
        run(&["dispatch"])?.code == 0,
        "the first dispatch failed".to_string(),
    )?;
    let first = run(&["ingest"])?;
    let said = format!("{}{}", first.out, first.err);
    check(
        first.code != 0 && said.contains("L1"),
        format!("the prose answer was not rejected: {said}"),
    )?;
    let again = run(&["dispatch"])?;
    check(
        again.code == 0 && again.out.contains("L1: ok"),
        format!("L1 was not sent again: {}", again.out),
    )?;
    let second = run(&["ingest"])?;
    check(
        second.code == 0,
        format!(
            "the second answer was not accepted: {}{}",
            second.out, second.err
        ),
    )?;
    let log = run(&["log"])?;
    check(
        log.out.contains("rejected (attempt 1)"),
        format!("the rejection is not in the diary: {}", log.out),
    )?;
    Ok("a signed-out tool, a rejected answer, a retry".to_string())
}

// ----------------------------------------------------------------------------- the stand-in tool

/// Act as `claude`, `codex` or `gemini`: read the packet on standard input, answer as the script
/// would, in that tool's output convention.
fn stand_in(state: &str) -> i32 {
    let me = std::env::current_exe().ok();
    let name = me
        .as_deref()
        .and_then(Path::file_stem)
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut prompt = String::new();
    let _ = std::io::stdin().read_to_string(&mut prompt);
    let listed = |var: &str| {
        std::env::var(var)
            .unwrap_or_default()
            .split(',')
            .any(|n| n == name)
    };
    if listed("COUNCIL_TEST_TOOL_DOWN") {
        eprintln!("not logged in: run `{name} login` first");
        return 3;
    }
    let reply = if prompt.contains("Reply with exactly the two letters") {
        "OK".to_string() // doctor --probe
    } else {
        let seat = script::seat_of(&prompt);
        let first_line = prompt.lines().next().unwrap_or("");
        let mark = Path::new(state).join(format!(
            "{seat}.{:x}",
            first_line.len() * 31 + first_line.bytes().map(usize::from).sum::<usize>()
        ));
        let first = !mark.exists();
        let _ = std::fs::write(&mark, "seen");
        let prose = std::env::var("COUNCIL_TEST_TOOL_PROSE")
            .unwrap_or_default()
            .split(',')
            .any(|s| s == seat);
        if first && prose {
            "Sure! I had a careful look at the packet and here are my thoughts in plain prose."
                .to_string()
        } else {
            script::answer(&prompt).unwrap_or_else(|| "I do not know this packet.".to_string())
        }
    };
    let args: Vec<String> = std::env::args().collect();
    match name.as_str() {
        // codex exec ... -o <file> ... -
        "codex" => {
            if let Some(out) = args
                .iter()
                .position(|a| a == "-o")
                .and_then(|i| args.get(i + 1))
            {
                let _ = std::fs::write(out, &reply);
            }
            println!("tokens used: 1234");
        }
        // gemini -o json -p <nudge>
        "gemini" => println!("{}", json!({"response": reply, "stats": {}})),
        // claude -p --output-format text
        _ => println!("{reply}"),
    }
    0
}

// ----------------------------------------------------------------------------- the runner

fn main() {
    if let Ok(state) = std::env::var("COUNCIL_TEST_TOOL") {
        let named_tool = std::env::current_exe()
            .ok()
            .and_then(|p| p.file_stem().map(|n| n.to_string_lossy().into_owned()))
            .is_some_and(|n| TOOLS.contains(&n.as_str()));
        if named_tool {
            std::process::exit(stand_in(&state));
        }
    }
    type Test = Box<dyn Fn() -> Result<String, String> + Send>;
    let mut tests: Vec<(String, Test)> = Vec::new();
    let mut cassettes: Vec<PathBuf> = std::fs::read_dir(repo().join("tests").join("cassettes"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    cassettes.sort();
    for file in cassettes {
        let name = format!(
            "recorded::{}",
            file.file_stem()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        );
        // recorded with POSIX paths: on Windows the played scenarios and the other suites stand in
        if cfg!(unix) {
            tests.push((name, Box::new(move || replay(&file))));
        }
    }
    tests.push(("played::lite".to_string(), Box::new(lite_played)));
    tests.push(("played::full".to_string(), Box::new(full_played)));
    tests.push(("played::cli_seats".to_string(), Box::new(cli_seats)));
    tests.push(("played::cli_trouble".to_string(), Box::new(cli_trouble)));
    let filters: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .collect();
    let listing = std::env::args().any(|a| a == "--list");
    tests.retain(|(name, _)| {
        filters.is_empty() || filters.iter().any(|f| name.contains(f.as_str()))
    });
    if listing {
        for (name, _) in &tests {
            println!("{name}: test");
        }
        return;
    }
    println!("\nrunning {} scenarios", tests.len());
    let started = std::time::Instant::now();
    // a few at a time: each scenario runs dozens of processes of its own
    let workers = std::thread::available_parallelism()
        .map_or(2, |n| n.get())
        .min(4);
    let queue = std::sync::Mutex::new(tests.into_iter());
    let results = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let next = queue.lock().expect("queue").next();
                let Some((name, test)) = next else { break };
                let result = test();
                match &result {
                    Ok(note) => println!("test {name} ... ok ({note})"),
                    Err(_) => println!("test {name} ... FAILED"),
                }
                results.lock().expect("results").push((name, result));
            });
        }
    });
    let results = results.into_inner().expect("results");
    let failed: Vec<&(String, Result<String, String>)> =
        results.iter().filter(|(_, r)| r.is_err()).collect();
    for (name, result) in &failed {
        println!(
            "\n---- {name} ----\n{}",
            result.as_ref().err().map(String::as_str).unwrap_or("")
        );
    }
    println!(
        "\ntest result: {}. {} passed; {} failed; finished in {:.1}s\n",
        if failed.is_empty() { "ok" } else { "FAILED" },
        results.len() - failed.len(),
        failed.len(),
        started.elapsed().as_secs_f64()
    );
    std::process::exit(i32::from(!failed.is_empty()));
}
