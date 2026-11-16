//! Give a seat a fresh context on any model family.
//!
//! ELI5: one small adapter per AI company. It calls the official command-line tools you already
//! use (claude, codex, gemini) or OpenRouter's API, and returns the answer as text.
//!
//! Which model actually runs. The registry (`models/lock.yaml`) names models the way Artificial
//! Analysis does, for example "GPT-6.1 Sol (high)". CLIs want their own ids. For a seat, in order:
//!
//! 1. an environment override: `COUNCIL_CLAUDE_MODEL` / `COUNCIL_OPENAI_MODEL` / `COUNCIL_GEMINI_MODEL`
//! 2. `native_ids` in `models/policy.yaml` (you maintain this small map)
//! 3. Claude only: the family alias (fable | opus | sonnet | haiku)
//! 4. the CLI's own default model, reported as "cli default" so the ledger stays honest
//!
//! Effort ("(high)" in the name) is passed to `claude --effort`. The codex and gemini CLIs get no
//! effort flag from here: set it in their own config if you need it.
//!
//! Data leaving your machine: every provider here sends the prompt to that company. Use mode `solo`
//! or a single-family fan-out for code you may not share across vendors.

use crate::core;
use crate::py::{self, Json};
use crate::val::{items, Get};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

/// The Claude family names a model name can carry.
pub const CLAUDE_ALIASES: [&str; 4] = ["fable", "opus", "sonnet", "haiku"];
/// Effort levels the claude CLI accepts.
pub const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];
const GEMINI_NUDGE: &str =
    "Follow the instructions above exactly. Reply with only the JSON they ask for.";
const OPENROUTER_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
/// Bytes of a tool's reply that are kept.
const REPLY_CAP: u64 = 32_000_000;
/// Bytes `parse_json` will look at before it gives up on a reply.
const PARSE_BUDGET: usize = 4_000_000;

const ELDER_INSTRUCTIONS: &str = r#"You are an independent Elder Council reviewer. Review the blinded packet below.
Rules: judge only against the rubric; ignore length and style; no nitpicks; flag blockers as S1.
If unsure, say so. Do not try to guess which model or person wrote a candidate.
Return ONLY JSON:
{"scores": {"<label>": {"<criterion>": 1-10}}, "verdict": {"<label>": "ship|revise|kill"},
 "preferred": "<label>", "confidence": 0.0-1.0,
 "top_risks": [{"risk": "...", "severity": "S1|S2|S3"}], "key_fix": "...", "own_idea": "...",
 "eli5": "<=20 words"}
"#;

static EFFORT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)[\s\x1c-\x1f]*\((max|xhigh|high|medium|low|minimal)\)[\s\x1c-\x1f]*$")
        .expect("valid")
});

/// A model name without its effort suffix: `"GPT-6.1 Sol (high)"` -> `"GPT-6.1 Sol"`.
pub fn base_name(model_name: &str) -> String {
    py::strip(&EFFORT_RE.replace_all(model_name, "")).to_string()
}

/// The effort a model name asks for, when the claude CLI knows it.
pub fn effort_of(model_name: &str) -> Option<String> {
    let effort = EFFORT_RE
        .captures(model_name)?
        .get(1)?
        .as_str()
        .to_lowercase();
    EFFORTS.contains(&effort.as_str()).then_some(effort)
}

/// The Claude family alias inside a model name.
pub fn claude_alias(model_name: &str) -> Option<&'static str> {
    let low = model_name.to_lowercase();
    CLAUDE_ALIASES.into_iter().find(|a| low.contains(a))
}

/// The provider that reaches a lab's models.
pub fn creator_provider(creator: &str) -> Option<&'static str> {
    match creator {
        "Anthropic" => Some("claude"),
        "OpenAI" => Some("openai"),
        "Google" => Some("gemini"),
        _ => None,
    }
}

/// The id to pass to the CLI (`null` = let the CLI choose), and how it was chosen.
pub fn native_id(
    provider: &str,
    model_name: Option<&str>,
    policy: &Value,
) -> (Value, &'static str) {
    if let Some(id) = std::env::var(format!("COUNCIL_{}_MODEL", provider.to_uppercase()))
        .ok()
        .filter(|v| !v.is_empty())
    {
        return (json!(id), "env override");
    }
    let name = model_name.unwrap_or("");
    if let Some(id) = policy.at("native_ids").get(base_name(name)) {
        return (id.clone(), "policy native_ids");
    }
    if provider == "claude" {
        if let Some(alias) = claude_alias(name) {
            return (json!(alias), "family alias");
        }
    }
    (Value::Null, "cli default")
}

/// The command line that sends one prompt to a provider's tool, or why there is none.
///
/// `tools` is what the seat may do while it works: `none` (judges read only their packet), `read`
/// (the project's files), `web` (search and open pages) or `research` (both). A seat that can open
/// web pages and read private files can be talked into sending one to the other, so the run loop
/// never asks for `research`: a researcher gets the web or the files, never both.
fn command(
    provider: &str,
    model: Option<&str>,
    effort: Option<&str>,
    tools: &str,
    out_file: &Path,
) -> Result<Vec<String>, String> {
    let model = model.filter(|m| !m.is_empty());
    if let Some(odd) = model.filter(|m| !crate::safe::plain_arg(m)) {
        // it becomes the value of an option of another program: it must not read as an option itself
        return Err(format!(
            "{} is not a model id",
            py::str_repr(&crate::safe::shown(py::head(odd, 60)))
        ));
    }
    let mut cmd: Vec<String> = Vec::new();
    let mut add = |parts: &[&str]| cmd.extend(parts.iter().map(|p| (*p).to_string()));
    match provider {
        "claude" => {
            add(&[
                "claude",
                "-p",
                "--output-format",
                "text",
                "--no-session-persistence",
            ]);
            if let Some(model) = model {
                add(&["--model", model]);
            }
            if let Some(effort) = effort.filter(|e| EFFORTS.contains(e)) {
                add(&["--effort", effort]);
            }
            match tools {
                "read" => add(&["--allowedTools", "Read,Grep,Glob"]),
                "web" => add(&["--allowedTools", "WebSearch,WebFetch"]),
                "research" => add(&["--allowedTools", "Read,Grep,Glob,WebSearch,WebFetch"]),
                _ => add(&["--tools", ""]),
            }
        } // the prompt arrives on stdin
        "openai" => {
            let out = out_file.display().to_string();
            add(&["codex", "exec", "--skip-git-repo-check", "-o", &out]);
            if let Some(model) = model {
                add(&["-m", model]);
            }
            add(&["-"]);
        }
        "gemini" => {
            add(&["gemini"]);
            if let Some(model) = model {
                add(&["-m", model]);
            }
            add(&["-o", "json", "-p", GEMINI_NUDGE]);
        }
        _ => return Err(format!("unknown provider: {provider}")),
    }
    Ok(cmd)
}

fn openrouter(prompt: &str, model: &str, timeout: u64) -> Result<String, String> {
    let key = std::env::var("OPENROUTER_API_KEY")
        .map_err(|_| "OPENROUTER_API_KEY is not set".to_string())?;
    let body = Json::LINE
        .dumps(&json!({"model": model, "messages": [{"role": "user", "content": prompt}]}));
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(timeout)))
        .build()
        .into();
    let mut response = agent
        .post(OPENROUTER_URL)
        .header("Authorization", &format!("Bearer {key}"))
        .header("Content-Type", "application/json")
        .send(body.as_bytes())
        .map_err(|e| e.to_string())?;
    let text = response
        .body_mut()
        .with_config()
        .limit(REPLY_CAP)
        .read_to_string()
        .map_err(|e| e.to_string())?;
    let reply: Value =
        serde_json::from_str(&text).map_err(|e| format!("the reply is not JSON: {e}"))?;
    match reply
        .at("choices")
        .get(0)
        .map(|c| c.at("message").at("content"))
    {
        Some(Value::String(content)) => Ok(content.clone()),
        Some(Value::Null) | None => Err(format!(
            "the reply holds no message: {}",
            py::head(&py::squash(&text), 300)
        )),
        Some(other) => Ok(py::to_str(other)),
    }
}

/// A folder of our own under the system's temporary folder, removed when it goes out of scope.
/// Its name cannot be guessed, it is created (never adopted: a folder or link someone put there
/// first is not used) and only its owner may enter it.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> std::io::Result<Scratch> {
        use std::hash::{BuildHasher as _, Hasher as _};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let mut last = None;
        for _ in 0..8 {
            // the standard library seeds this hasher from the system's random source
            let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
            hasher.write_usize(NEXT.fetch_add(1, Ordering::Relaxed));
            hasher.write_u32(std::process::id());
            let dir = std::env::temp_dir().join(format!("council-{:016x}", hasher.finish()));
            // only Unix has a mode to set, so only there is the builder changed
            #[cfg_attr(not(unix), allow(unused_mut))]
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt as _;
                builder.mode(0o700);
            }
            match builder.create(&dir) {
                Ok(()) => return Ok(Scratch(dir)),
                Err(err) => last = Some(err),
            }
        }
        Err(last.unwrap_or_else(|| std::io::Error::other("no temporary folder")))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Text a tool printed: UTF-8 with stray bytes replaced, and `\n` line ends.
fn printed(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    if text.contains('\r') {
        text.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        text.into_owned()
    }
}

struct Finished {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// Run a tool with `input` on its standard input. `Ok(None)` = it did not finish in time (and was stopped).
fn run_tool(
    exe: &Path,
    args: &[String],
    input: &str,
    timeout: u64,
) -> std::io::Result<Option<Finished>> {
    let mut child = Command::new(exe)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdin = child.stdin.take();
    let text = input.to_string();
    let writer = std::thread::spawn(move || {
        if let Some(mut pipe) = stdin {
            let _ = pipe.write_all(text.as_bytes()); // a tool that exits early closes the pipe: not an error here
        }
    });
    fn drain(
        pipe: Option<impl std::io::Read + Send + 'static>,
    ) -> std::thread::JoinHandle<Vec<u8>> {
        std::thread::spawn(move || {
            use std::io::Read as _;
            let mut buffer = Vec::new();
            if let Some(mut pipe) = pipe {
                // keep the first 32 MB and let the rest run off, so a tool that never stops
                // printing fills neither memory nor its pipe
                let _ = pipe.by_ref().take(REPLY_CAP).read_to_end(&mut buffer);
                let _ = std::io::copy(&mut pipe, &mut std::io::sink());
            }
            buffer
        })
    }
    let (out, err) = (drain(child.stdout.take()), drain(child.stderr.take()));
    let deadline = Instant::now() + Duration::from_secs(timeout);
    let status = loop {
        match child.try_wait()? {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(None); // the reader threads end when the pipes close; nobody waits for them
            }
            None => std::thread::sleep(Duration::from_millis(40)),
        }
    };
    let _ = writer.join();
    let stdout = printed(&out.join().unwrap_or_default());
    let stderr = printed(&err.join().unwrap_or_default());
    Ok(Some(Finished {
        code: status.code(),
        stdout,
        stderr,
    }))
}

/// Send one prompt in a fresh context. `model` is a native id or alias (see [`native_id`]) or `None`.
///
/// `tools`: "none" (judges), "read" (experts may read project files) or "research" (also open web
/// pages; only the claude CLI is given the web tools here: codex and gemini run with whatever web
/// access their own settings allow).
///
/// Returns `{"ok", "provider", "model", "text" | "error", "command"?}`.
pub fn ask(
    provider: &str,
    prompt: &str,
    model: Option<&str>,
    effort: Option<&str>,
    timeout: u64,
    dry_run: bool,
    tools: &str,
) -> Value {
    let model = model.filter(|m| !m.is_empty());
    if provider == "openrouter" {
        if dry_run {
            return json!({"ok": true, "provider": provider, "model": model, "dry_run": true, "command": format!("POST {OPENROUTER_URL}")});
        }
        let has_key = std::env::var("OPENROUTER_API_KEY").is_ok_and(|k| !k.is_empty());
        let Some(model) = model.filter(|_| has_key) else {
            return json!({"ok": false, "provider": provider, "error": "need OPENROUTER_API_KEY and a model slug"});
        };
        return match openrouter(prompt, model, timeout) {
            Ok(text) => json!({"ok": true, "provider": provider, "model": model, "text": text}),
            Err(error) => json!({"ok": false, "provider": provider, "error": error}),
        };
    }
    let scratch = match Scratch::new() {
        Ok(scratch) => scratch,
        Err(err) => {
            return json!({"ok": false, "provider": provider, "error": format!("no temporary folder: {err}")})
        }
    };
    let out_file = scratch.0.join("last.txt");
    let cmd = match command(provider, model, effort, tools, &out_file) {
        Ok(cmd) => cmd,
        Err(why) => return json!({"ok": false, "provider": provider, "error": why}),
    };
    let shown_model = model.unwrap_or("cli default");
    if dry_run {
        return json!({"ok": true, "provider": provider, "model": shown_model, "dry_run": true, "command": cmd,
                      "stdin": format!("<prompt, {} chars>", py::len(prompt))});
    }
    crate::diag!(Debug, "seat", "running {}", cmd.join(" "); "prompt_chars" => py::len(prompt), "tools" => tools);
    let Ok(exe) = which::which(&cmd[0]) else {
        return json!({"ok": false, "provider": provider, "error": format!("CLI not found: {}", cmd[0]), "command": cmd});
    };
    // the resolved path: on Windows npm tools are .cmd files
    let done = match run_tool(&exe, &cmd[1..], prompt, timeout) {
        Ok(Some(done)) => done,
        Ok(None) => {
            return json!({"ok": false, "provider": provider, "error": format!("timeout after {timeout}s")})
        }
        Err(err) => {
            return json!({"ok": false, "provider": provider, "error": format!("could not run {}: {err}", cmd[0]), "command": cmd})
        }
    };
    let mut text = if crate::safe::plain_file(&out_file, REPLY_CAP).is_some() {
        printed(&std::fs::read(&out_file).unwrap_or_default())
    } else {
        done.stdout.clone()
    };
    if provider == "gemini" {
        if let Ok(Value::Object(reply)) = serde_json::from_str::<Value>(&done.stdout) {
            text = match reply.get("response") {
                Some(Value::String(s)) => s.clone(),
                Some(other) => py::to_str(other),
                None => done.stdout.clone(),
            };
        }
    }
    let ok = done.code == Some(0) && !py::strip(&text).is_empty();
    crate::diag!(Info, "seat", "{provider} ({shown_model}) {}", if ok { "answered" } else { "failed" };
                 "exit" => done.code, "reply_chars" => py::len(&text));
    let mut out = json!({"ok": ok, "provider": provider, "model": shown_model, "text": text});
    if !ok {
        let why = if done.stderr.is_empty() {
            "empty reply"
        } else {
            done.stderr.as_str()
        };
        out["error"] = json!(py::tail(why, py::len(why).saturating_sub(2000))); // the last 2000 characters
    }
    out
}

/// First JSON object in a reply, tolerant of code fences and prose around it. The search is
/// bounded: a reply made of a million open braces is given up on, not scanned a million times.
pub fn parse_json(text: &str) -> Option<Value> {
    match serde_json::from_str::<Value>(text) {
        Ok(obj @ Value::Object(_)) => return Some(obj),
        Ok(_) => return None,
        Err(_) => {}
    }
    let bytes = text.as_bytes();
    let mut start = text.find('{');
    let mut budget = PARSE_BUDGET;
    while let Some(from) = start {
        let (mut depth, mut in_str, mut esc) = (0i64, false, false);
        for (i, &ch) in bytes.iter().enumerate().skip(from) {
            if budget == 0 {
                return None;
            }
            budget -= 1;
            if in_str {
                esc = ch == b'\\' && !esc;
                if ch == b'"' && !esc {
                    in_str = false;
                }
                continue;
            }
            match ch {
                b'"' => in_str = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        if let Ok(obj @ Value::Object(_)) =
                            serde_json::from_str::<Value>(&text[from..=i])
                        {
                            return Some(obj);
                        }
                        break;
                    }
                }
                _ => {}
            }
        }
        start = text[from + 1..].find('{').map(|at| from + 1 + at);
    }
    None
}

/// Run `work` on every item, at most `limit` at a time, and return the results in the items' order.
pub fn in_parallel<T: Sync, R: Send>(
    items_in: &[T],
    limit: usize,
    work: impl Fn(&T) -> R + Sync,
) -> Vec<R> {
    let next = AtomicUsize::new(0);
    let slots: Vec<std::sync::Mutex<Option<R>>> = items_in
        .iter()
        .map(|_| std::sync::Mutex::new(None))
        .collect();
    std::thread::scope(|scope| {
        for _ in 0..limit.clamp(1, items_in.len().max(1)) {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(item) = items_in.get(i) else { break };
                let result = work(item);
                if let Ok(mut slot) = slots[i].lock() {
                    *slot = Some(result);
                }
            });
        }
    });
    slots
        .into_iter()
        .filter_map(|slot| slot.into_inner().ok().flatten())
        .collect()
}

/// Stand-alone elder review: the same blinded packet to each lab in parallel, then a summary.
/// Inside a run, use `council next` / `council dispatch` instead: they also blind, record and validate.
/// `save`: also write each provider's answer as `<out_dir>/<provider>.json`.
pub fn elder_review(
    packet: &str,
    profile: &str,
    providers: &[String],
    out_dir: Option<&Path>,
    dry_run: bool,
    save: bool,
) -> Value {
    let timeout = 900;
    let default: Vec<String> = ["claude", "openai", "gemini"]
        .iter()
        .map(|p| (*p).to_string())
        .collect();
    let providers = if providers.is_empty() {
        &default[..]
    } else {
        providers
    };
    let picks = core::models_for("crosslab", profile);
    let policy = core::policy();
    let mut by_provider: Map<String, Value> = Map::new();
    for elder in picks.list("elders").iter().filter(|e| e.is_object()) {
        if let Some(provider) = creator_provider(elder.s("creator")) {
            by_provider.insert(provider.to_string(), elder.at("model").clone());
        }
    }
    let prompt = format!("{ELDER_INSTRUCTIONS}\n\n{packet}");
    // a folder the person chose is theirs to choose; the default one is checked like any other write
    let named = out_dir.is_some_and(|d| d != Path::new(".council/elders"));
    let out_dir = out_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(".council/elders"));
    let folder = if !save {
        Ok(())
    } else if named {
        std::fs::create_dir_all(&out_dir).map_err(anyhow::Error::from)
    } else {
        crate::safe::make_dir(&out_dir).map(|_| ())
    };
    let results: Vec<Value> = in_parallel(providers, providers.len(), |p| {
        let name = by_provider.get(p.as_str()).and_then(Value::as_str);
        let (id, how) = native_id(p, name, &policy);
        let effort = name.and_then(effort_of);
        let mut res = ask(
            p,
            &prompt,
            id.as_str(),
            effort.as_deref(),
            timeout,
            dry_run,
            "none",
        );
        res["requested"] = json!(name);
        res["model_source"] = json!(how);
        res
    });
    let mut by_name: Map<String, Value> = Map::new(); // a provider named twice counts once, as its last call
    for (p, r) in providers.iter().zip(results) {
        by_name.insert(p.clone(), r);
    }
    let mut reviews: Vec<Value> = Vec::new();
    let mut unwritten: Vec<String> = folder
        .err()
        .map(|e| format!("{}: {e:#}", out_dir.display()))
        .into_iter()
        .collect();
    for (p, r) in &by_name {
        let file = out_dir.join(format!("{p}.json"));
        let saved = if !save {
            Ok(()) // the caller gets the answers back and keeps them where it likes
        } else if named {
            crate::safe::write_named(&file, &Json::ONE.dumps(r))
        } else {
            core::write_text(&file, &Json::ONE.dumps(r))
        };
        if let Err(err) = saved {
            unwritten.push(format!("{p}.json: {err:#}"));
        }
        if r.truthy("ok") && !r.truthy("dry_run") {
            if let Some(review) = parse_json(r.s("text")) {
                reviews.push(crate::safe::cleaned(review));
            }
        }
    }
    let preferred: Vec<Value> = reviews
        .iter()
        .map(|v| v.at("preferred"))
        .filter(|p| py::truthy(p))
        .cloned()
        .collect();
    let s1: Vec<Value> = reviews
        .iter()
        .flat_map(|v| v.list("top_risks").iter())
        .filter(|risk| risk.s("severity") == "S1")
        .cloned()
        .collect();
    let mut distinct: Vec<&Value> = Vec::new();
    for p in &preferred {
        if !distinct.contains(&p) {
            distinct.push(p);
        }
    }
    let split = distinct.len() > 1;
    let missing: Vec<&String> = by_name
        .iter()
        .filter(|(_, r)| !r.truthy("ok"))
        .map(|(p, _)| p)
        .collect();
    let results: Map<String, Value> = by_name
        .iter()
        .map(|(p, r)| {
            (
                p.clone(),
                Value::Object(
                    items(r)
                        .filter(|(k, _)| k.as_str() != "text")
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                ),
            )
        })
        .collect();
    let stand_in_note = if missing.is_empty() {
        Value::Null
    } else {
        json!("Replace missing providers with labelled stand-ins (another strong model in a fresh context)")
    };
    let next = if split || !s1.is_empty() {
        "Elders disagree or an S1 is open: stop and ask the requester ONE targeted question."
    } else {
        "Proceed: feed elder feedback into the next loop."
    };
    let mut out = json!({"ok": missing.is_empty() || dry_run, "dry_run": dry_run, "results": results, "preferred": preferred, "split": split,
                         "s1_risks": s1, "missing": missing, "stand_in_note": stand_in_note, "next": next});
    if !unwritten.is_empty() {
        out["not_written"] = json!(unwritten);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_names_split_into_name_effort_and_alias() {
        assert_eq!(base_name("GPT-6.1 Sol (high)"), "GPT-6.1 Sol");
        assert_eq!(base_name("  Claude Fable 5.2 (MAX) "), "Claude Fable 5.2");
        assert_eq!(base_name("Gemini 4 Pro"), "Gemini 4 Pro");
        assert_eq!(effort_of("GPT-6.1 Sol (xhigh)").as_deref(), Some("xhigh"));
        assert_eq!(effort_of("GPT-6.1 Sol (minimal)"), None);
        assert_eq!(effort_of("GPT-6.1 Sol"), None);
        assert_eq!(claude_alias("Claude Opus 5 (high)"), Some("opus"));
        assert_eq!(claude_alias("GPT-6.1"), None);
        assert_eq!(creator_provider("Google"), Some("gemini"));
    }

    #[test]
    fn the_id_for_a_cli_comes_from_the_policy_then_the_family() {
        let policy = json!({"native_ids": {"GPT-6.1 Sol": "gpt-6.1-sol"}});
        assert_eq!(
            native_id("openai", Some("GPT-6.1 Sol (high)"), &policy),
            (json!("gpt-6.1-sol"), "policy native_ids")
        );
        assert_eq!(
            native_id("claude", Some("Claude Fable 5.2 (max)"), &policy),
            (json!("fable"), "family alias")
        );
        assert_eq!(
            native_id("gemini", Some("Gemini 4 Pro"), &policy),
            (Value::Null, "cli default")
        );
        assert_eq!(
            native_id("gemini", None, &Value::Null),
            (Value::Null, "cli default")
        );
    }

    #[test]
    fn commands_are_built_per_tool() {
        let out = Path::new("/tmp/x/last.txt");
        assert_eq!(
            command("claude", Some("opus"), Some("high"), "research", out).unwrap().join(" "),
            "claude -p --output-format text --no-session-persistence --model opus --effort high --allowedTools Read,Grep,Glob,WebSearch,WebFetch"
        );
        assert_eq!(
            command("claude", None, Some("minimal"), "none", out).unwrap()[5..],
            ["--tools".to_string(), String::new()]
        );
        assert_eq!(
            command("openai", Some("gpt"), None, "none", out)
                .unwrap()
                .join(" "),
            "codex exec --skip-git-repo-check -o /tmp/x/last.txt -m gpt -"
        );
        assert_eq!(
            command("gemini", None, None, "read", out).unwrap()[..4].join(" "),
            "gemini -o json -p"
        );
        assert!(command("other", None, None, "none", out).is_err());
        assert_eq!(
            command("claude", None, None, "web", out).unwrap()[5..],
            [
                "--allowedTools".to_string(),
                "WebSearch,WebFetch".to_string()
            ]
        );
        // a model id is an option's value for another program: it may not read as an option
        for odd in ["--dangerously-skip-permissions", "-c", "a\nb"] {
            for provider in ["claude", "openai", "gemini"] {
                assert!(
                    command(provider, Some(odd), None, "none", out).is_err(),
                    "{provider} {odd}"
                );
            }
        }
    }

    #[test]
    fn the_first_json_object_is_found_in_prose_and_fences() {
        assert_eq!(parse_json(r#"{"a": 1}"#), Some(json!({"a": 1})));
        assert_eq!(
            parse_json("Here:\n```json\n{\"a\": {\"b\": \"}\"}}\n```\nDone {x}"),
            Some(json!({"a": {"b": "}"}}))
        );
        assert_eq!(
            parse_json("{not json} then {\"ok\": true}"),
            Some(json!({"ok": true}))
        );
        assert_eq!(
            parse_json(r#"say "{" then {"q": "a \"quoted\" } brace"}"#),
            Some(json!({"q": "a \"quoted\" } brace"}))
        );
        assert_eq!(parse_json("[{\"a\": 1}]"), None);
        assert_eq!(parse_json("no object here"), None);
        assert_eq!(parse_json(""), None);
        // made to be slow: every brace opens an object that never closes
        let started = Instant::now();
        assert_eq!(parse_json(&"{".repeat(1_500_000)), None);
        assert_eq!(parse_json(&"{\"a\":".repeat(200_000)), None);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[cfg(unix)]
    #[test]
    fn the_scratch_folder_is_private_and_never_adopted() {
        use std::os::unix::fs::PermissionsExt as _;
        let (a, b) = (Scratch::new().unwrap(), Scratch::new().unwrap());
        assert_ne!(a.0, b.0);
        assert_eq!(
            std::fs::metadata(&a.0).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let kept = a.0.clone();
        drop(a);
        assert!(!kept.exists() && b.0.exists());
    }

    #[test]
    fn a_dry_run_names_the_command_and_calls_nothing() {
        let res = ask("claude", "héllo", Some("opus"), None, 5, true, "none");
        assert_eq!(res["stdin"], json!("<prompt, 5 chars>"));
        assert_eq!(res["command"][0], json!("claude"));
        let res = ask("openrouter", "x", None, None, 5, true, "none");
        assert_eq!(res["model"], Value::Null);
        assert_eq!(
            ask("nobody", "x", None, None, 5, false, "none")["ok"],
            json!(false)
        );
    }

    #[test]
    fn work_in_parallel_keeps_its_order() {
        let doubled = in_parallel(&[1, 2, 3, 4, 5], 2, |n| n * 2);
        assert_eq!(doubled, vec![2, 4, 6, 8, 10]);
        assert!(in_parallel::<i32, i32>(&[], 4, |n| *n).is_empty());
    }
}
