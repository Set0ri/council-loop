//! Diagnostics for people who operate the tool, and the event log of a run.
//!
//! ELI5: two notebooks. One is the tool talking to whoever runs it ("I read your settings from
//! here, I picked this mode because..."): off unless you ask, on standard error or in a file. The
//! other is a run's diary (`events.jsonl` in the run's folder): one line per step taken and per
//! seat called, kept with the run, so "what happened, when, and how long did it take" has an
//! answer after the fact.
//!
//! Diagnostics
//!
//! * `COUNCIL_LOG=error|warn|info|debug|trace` (or `-v`, `-vv`, `--log-level`) sets how much is
//!   said. The default is `warn`. `off` silences it.
//! * `COUNCIL_LOG_FILE=<path>` appends to a file of your choosing; without it, standard error.
//! * `COUNCIL_LOG_FORMAT=json` writes one JSON object per line (`t`, `level`, `target`, `msg`,
//!   then the fields) for other programs to read.
//! * Nothing secret is written: keys held in the environment and anything shaped like a key or a
//!   bearer token are replaced by `***` before a line leaves this module.
//!
//! Standard output is never used here: it carries the command's answer (or, for `council mcp`,
//! protocol messages only).

use crate::py::Json;
use regex::Regex;
use serde_json::{json, Map, Value};
use std::io::Write as _;
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{LazyLock, Mutex};

/// How much the tool says about what it is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Nothing.
    Off = 0,
    /// Something failed.
    Error = 1,
    /// Something was ignored or looks wrong (the default).
    Warn = 2,
    /// What was decided, and why.
    Info = 3,
    /// What was read, written and run.
    Debug = 4,
    /// Everything, timings included.
    Trace = 5,
}

impl Level {
    /// The level a name stands for (`warning` and MCP's `notice`, `critical` and the like included).
    pub fn parse(name: &str) -> Option<Level> {
        Some(match name.trim().to_ascii_lowercase().as_str() {
            "off" | "none" | "0" => Level::Off,
            "error" | "critical" | "alert" | "emergency" => Level::Error,
            "warn" | "warning" => Level::Warn,
            "info" | "notice" => Level::Info,
            "debug" => Level::Debug,
            "trace" => Level::Trace,
            _ => return None,
        })
    }

    /// The level's name.
    pub fn name(self) -> &'static str {
        match self {
            Level::Off => "off",
            Level::Error => "error",
            Level::Warn => "warn",
            Level::Info => "info",
            Level::Debug => "debug",
            Level::Trace => "trace",
        }
    }
}

const UNSET: u8 = u8::MAX;
static LEVEL: AtomicU8 = AtomicU8::new(UNSET);

/// What a program that embeds this tool (the MCP server) does with a diagnostic line.
type Listener = Box<dyn Fn(Level, &str, &str, &Value) + Send + Sync>;
static LISTENER: Mutex<Option<Listener>> = Mutex::new(None);
/// What the MCP server does when a long step reports that one more part of it is done.
type Progress = Box<dyn Fn(&str) + Send + Sync>;
static PROGRESS: Mutex<Option<Progress>> = Mutex::new(None);

/// The level in force: what was set, else `COUNCIL_LOG`, else `warn`.
pub fn level() -> Level {
    let raw = LEVEL.load(Ordering::Relaxed);
    if raw != UNSET {
        return from_u8(raw);
    }
    let found = std::env::var("COUNCIL_LOG")
        .ok()
        .and_then(|v| Level::parse(&v))
        .unwrap_or(Level::Warn);
    LEVEL.store(found as u8, Ordering::Relaxed);
    found
}

fn from_u8(raw: u8) -> Level {
    match raw {
        0 => Level::Off,
        1 => Level::Error,
        2 => Level::Warn,
        3 => Level::Info,
        4 => Level::Debug,
        _ => Level::Trace,
    }
}

/// Set the level for the rest of the process (a flag beats the environment).
pub fn set_level(level: Level) {
    LEVEL.store(level as u8, Ordering::Relaxed);
}

/// Would a line of this level be written? Ask before building an expensive message.
pub fn enabled(at: Level) -> bool {
    at != Level::Off && at <= level()
}

/// Send every diagnostic line to `listener` as well (the MCP server forwards them to its client).
pub fn listen(listener: Option<Listener>) {
    if let Ok(mut held) = LISTENER.lock() {
        *held = listener;
    }
}

/// Call `callback` whenever a long step reports progress, until this is called with `None`.
pub fn on_progress(callback: Option<Progress>) {
    if let Ok(mut held) = PROGRESS.lock() {
        *held = callback;
    }
}

/// One more part of a long step is done (a seat answered, say).
pub fn progress(message: &str) {
    if let Ok(held) = PROGRESS.lock() {
        if let Some(callback) = held.as_ref() {
            callback(message);
        }
    }
}

static SECRET_SHAPES: LazyLock<[(Regex, &'static str); 4]> = LazyLock::new(|| {
    [
        (
            Regex::new(
                r#"(?i)\b(authorization)(["']?\s*[=:]\s*["']?)(?:(?:bearer|basic|token)\s+)?[^\s"',;]{4,}"#,
            )
            .expect("valid"),
            "$1$2***",
        ),
        (
            Regex::new(r"(?i)\b(bearer)\s+[A-Za-z0-9._~+/=-]{8,}").expect("valid"),
            "$1 ***",
        ),
        (
            Regex::new(r"\b(sk|pk|rk|ghp|gho|ghs|xox[abp])[-_][A-Za-z0-9_-]{10,}").expect("valid"),
            "$1-***",
        ),
        (
            Regex::new(
                r#"(?i)\b(api[_-]?key|access[_-]?token|auth[_-]?token|token|secret|password|passwd)(["']?\s*[=:]\s*["']?)[^\s"',;]{4,}"#,
            )
            .expect("valid"),
            "$1$2***",
        ),
    ]
});

/// Environment variables whose values must never be written anywhere.
const SECRET_VARS: [&str; 8] = [
    "OPENROUTER_API_KEY",
    "AA_API_KEY",
    "ANTHROPIC_API_KEY",
    "OPENAI_API_KEY",
    "CODEX_API_KEY",
    "GEMINI_API_KEY",
    "GOOGLE_API_KEY",
    "CLAUDE_CODE_OAUTH_TOKEN",
];

/// `text` with secrets replaced by `***`: the values of the key variables this tool knows, and
/// anything shaped like a key, a bearer token or a `password=...` pair.
pub fn redact(text: &str) -> String {
    let mut out = text.to_string();
    for name in SECRET_VARS {
        if let Ok(value) = std::env::var(name) {
            if value.len() >= 8 && out.contains(&value) {
                out = out.replace(&value, "***");
            }
        }
    }
    for (shape, with) in SECRET_SHAPES.iter() {
        if shape.is_match(&out) {
            out = shape.replace_all(&out, *with).into_owned();
        }
    }
    out
}

fn sink(line: &str) {
    match std::env::var_os("COUNCIL_LOG_FILE").filter(|v| !v.is_empty()) {
        Some(path) => {
            // a file you named: appended to, created for you alone when it is new
            let mut options = std::fs::OpenOptions::new();
            options.append(true).create(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt as _;
                options.mode(0o600);
            }
            if let Ok(mut file) = options.open(path) {
                let _ = writeln!(file, "{line}");
            }
        }
        None => crate::out::error_line(line),
    }
}

/// Write one diagnostic line, if the level in force lets it through.
pub fn emit(at: Level, target: &str, message: &str, fields: Value) {
    if !enabled(at) {
        return;
    }
    let message = redact(message);
    let fields = match fields {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| match v {
                    Value::String(s) => (k, Value::String(redact(&s))),
                    other => (k, other),
                })
                .collect(),
        ),
        _ => json!({}),
    };
    if let Ok(held) = LISTENER.lock() {
        if let Some(listener) = held.as_ref() {
            listener(at, target, &message, &fields);
        }
    }
    let json_lines = std::env::var("COUNCIL_LOG_FORMAT").is_ok_and(|f| f == "json");
    let line = if json_lines {
        let mut out = Map::new();
        out.insert(
            "t".into(),
            json!(crate::clock::now().format("%Y-%m-%dT%H:%M:%S").to_string()),
        );
        out.insert("level".into(), json!(at.name()));
        out.insert("target".into(), json!(target));
        out.insert("msg".into(), json!(message));
        for (k, v) in crate::val::items(&fields) {
            out.insert(k.clone(), v.clone());
        }
        Json::LINE.dumps(&Value::Object(out))
    } else {
        let mut line = format!("[council {} {target}] {message}", at.name());
        for (k, v) in crate::val::items(&fields) {
            let shown = match v {
                Value::String(s) => s.clone(),
                other => Json::LINE.dumps(other),
            };
            line.push_str(&format!(" · {k}={shown}"));
        }
        line
    };
    sink(&line);
}

/// A diagnostic line: `diag!(Info, "gate", "tier {tier} because {why}"; "tripwires" => hits)`.
/// The message is built only when the level lets it through.
#[macro_export]
macro_rules! diag {
    ($level:ident, $target:expr, $($fmt:expr),+ $(; $($key:expr => $value:expr),* )?) => {
        if $crate::log::enabled($crate::log::Level::$level) {
            $crate::log::emit(
                $crate::log::Level::$level,
                $target,
                &format!($($fmt),+),
                serde_json::json!({ $($($key: $value),*)? }),
            );
        }
    };
}

// ----------------------------------------------------------------------------- a run's event log

/// The name of a run's event log, inside the run's folder.
pub const EVENTS: &str = "events.jsonl";

/// Milliseconds a step took. Zero when the clock is frozen (`COUNCIL_NOW`), so that a recorded
/// run reads the same every time it is replayed.
pub fn took(started: std::time::Instant) -> u64 {
    if std::env::var_os("COUNCIL_NOW").is_some() {
        0
    } else {
        u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

/// Append one event to a run's diary. `event` is an object; the time is added here. A diary that
/// cannot be written never stops a run: the failure is a diagnostic, nothing more.
pub fn record(run_dir: &Path, event: Value) {
    let mut line = Map::new();
    line.insert(
        "t".into(),
        json!(crate::clock::now().format("%Y-%m-%dT%H:%M:%S").to_string()),
    );
    for (k, v) in crate::val::items(&event) {
        let v = match v {
            Value::String(s) => Value::String(redact(s)),
            other => other.clone(),
        };
        line.insert(k.clone(), v);
    }
    let text = Json::LINE.dumps(&Value::Object(line));
    if let Err(err) = crate::safe::append_line(&run_dir.join(EVENTS), &text) {
        crate::diag!(
            Warn,
            "events",
            "the run's event log could not be written: {err:#}"
        );
    }
}

/// The events of a run, oldest first. Lines that are not JSON objects are skipped.
pub fn events(run_dir: &Path) -> Vec<Value> {
    crate::safe::read_own(&run_dir.join(EVENTS))
        .map(|text| {
            text.lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .filter(Value::is_object)
                .map(crate::safe::cleaned)
                .collect()
        })
        .unwrap_or_default()
}

/// One event as a line a person reads: time, what, and how it went.
pub fn event_line(event: &Value) -> String {
    use crate::val::Get as _;
    let time = event.s("t").rsplit('T').next().unwrap_or("");
    let secs = |ms: i64| {
        if ms >= 1000 {
            format!("{:.1} s", ms as f64 / 1000.0)
        } else {
            format!("{ms} ms")
        }
    };
    let outcome = if event.truthy("ok") { "ok" } else { "FAILED" };
    let took = event.int("ms").map(secs).unwrap_or_default();
    let what = match event.s("event") {
        "verb" => {
            let phase = event.list("phase");
            let moved = match (
                phase.first().and_then(Value::as_str),
                phase.get(1).and_then(Value::as_str),
            ) {
                (Some(a), Some(b)) if a != b => format!("{a} -> {b}"),
                (_, Some(b)) => b.to_string(),
                _ => String::new(),
            };
            format!("{:<10} {:<22} {outcome:<6} {took}", event.s("verb"), moved)
        }
        "seat_call" => format!(
            "seat {:<5} {:<22} {outcome:<6} {took}  {} -> {} chars",
            event.s("seat"),
            crate::py::head(
                &format!("{} {}", event.s("provider"), event.show("model")),
                22
            ),
            event.show("prompt_chars"),
            event.show("reply_chars")
        ),
        "seat_output" => format!(
            "seat {:<5} {:<22} {}",
            event.s("seat"),
            event.s("state"),
            crate::py::head(
                &event
                    .list("errors")
                    .iter()
                    .map(crate::py::to_str)
                    .collect::<Vec<_>>()
                    .join("; "),
                100
            )
        ),
        other => format!("{other:<10} {}", crate::py::head(&event.show("text"), 110)),
    };
    let mut line = format!("{time}  {}", what.trim_end());
    if event.truthy("error") {
        line.push_str(&format!(
            "  ({})",
            crate::py::head(&event.show("error"), 120)
        ));
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_are_named_and_ordered() {
        assert_eq!(Level::parse("WARNING"), Some(Level::Warn));
        assert_eq!(Level::parse("notice"), Some(Level::Info));
        assert_eq!(Level::parse("loud"), None);
        assert!(Level::Error < Level::Debug && Level::Off < Level::Error);
        assert_eq!(from_u8(Level::Trace as u8), Level::Trace);
        assert_eq!(Level::Debug.name(), "debug");
    }

    #[test]
    fn secrets_never_reach_a_log_line() {
        assert_eq!(
            redact("POST /v1 Authorization: Bearer abcdEFGH1234.5678-xyz ok"),
            "POST /v1 Authorization: *** ok"
        );
        assert_eq!(
            redact("curl -H 'Bearer abcdEFGH12345678'"),
            "curl -H 'Bearer ***'"
        );
        assert_eq!(
            redact("key sk-proj_ABCDEFGHIJKLMNOP1234 used"),
            "key sk-*** used"
        );
        assert_eq!(redact("api_key=abcd1234efgh, next"), "api_key=***, next");
        assert_eq!(
            redact(r#"{"password": "hunter2hunter2"}"#),
            r#"{"password": "***"}"#
        );
        assert_eq!(redact("token: ghp_abcdefghijklmnop123456"), "token: ***");
        // ordinary text is left alone
        for plain in [
            "the token budget is 700 tokens",
            "secret santa is on Friday",
            "tier=lite mode=fanout:claude",
            "password policy: 12 characters",
        ] {
            assert_eq!(redact(plain), plain);
        }
    }

    #[test]
    fn events_read_as_lines() {
        let verb = json!({"t": "2026-11-13T12:00:04", "event": "verb", "verb": "ingest", "ok": true, "ms": 12,
                          "phase": ["ideate", "merge"]});
        assert_eq!(
            event_line(&verb),
            "12:00:04  ingest     ideate -> merge        ok     12 ms"
        );
        let call = json!({"t": "2026-11-13T12:01:10", "event": "seat_call", "seat": "EA", "provider": "openai",
                          "model": "gpt-x", "ok": false, "ms": 48211, "prompt_chars": 5120, "reply_chars": 0,
                          "error": "timeout after 900s"});
        assert_eq!(
            event_line(&call),
            "12:01:10  seat EA    openai gpt-x           FAILED 48.2 s  5120 -> 0 chars  (timeout after 900s)"
        );
    }
}
