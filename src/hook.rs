//! `council hook`: the Claude Code hook that guards locked files and keeps council seats in their lane.
//!
//! ELI5: the answer key is locked in a drawer. The AI can read it but not change it. And each
//! member of the council may touch only its own sheet of paper.
//!
//! One command serves two hook events (see `hooks/hooks.json`):
//!
//! * **PreToolUse** (before a tool runs) denies
//!   - an edit of a locked file, or a shell command that plainly removes, moves or overwrites one;
//!   - `council lock-tests` while locks exist (re-locking would bless a changed answer key);
//!   - a council seat stepping out of its lane: a seat writes only its own output file, reads
//!     nothing in the run's folders but its packet, and never opens the file that says who wrote
//!     which card. A researcher (the seat that reads the web) reads no project file at all.
//! * **PostToolUse** (after a shell command ran) compares every locked file with its lock and tells
//!   the model when one no longer matches. This is the check a shell command cannot talk its way
//!   around: it looks at the files, not at the command.
//!
//! Files are locked with `council lock-tests`. Unlock: the project owner sets
//! `COUNCIL_ALLOW_LOCKED=1` for the session (needed to run `council lock-tests` again after an
//! approved test change), or the change goes through a PR labelled council-unlock. The hook is a
//! seat belt and can be bypassed; CI running `council verify-locks --against origin/<base>` is the
//! real lock.
//!
//! The hook reads the event as JSON on standard input. To object, it prints one JSON object; to
//! allow, it prints nothing. It always exits 0: a hook that fails must never block the session.

use crate::py::{self, Json};
use crate::val::Get;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::io::Read as _;
use std::path::{Path, PathBuf};

/// The agents this kit defines. A hook event that comes from one of them carries its name.
const SEATS: [&str; 6] = [
    "council-expert",
    "council-red-team",
    "council-elder",
    "council-researcher",
    "council-mapper",
    "council-historian",
];

/// Bytes of an event that are read. A `Write` event carries the whole file, so this is generous.
const MAX_EVENT: u64 = 256_000_000;

/// The first truthy text among some fields of the tool input.
fn first_text(input: &Value, keys: &[&str]) -> String {
    keys.iter()
        .map(|k| input.at(k))
        .find(|v| py::truthy(v))
        .map(py::to_str)
        .unwrap_or_default()
}

/// A path from an event, made absolute against the folder the tool call runs in.
fn absolute(text: &str, base: &Path) -> PathBuf {
    let p = py::path(text);
    if p.is_absolute() {
        p
    } else {
        base.join(p)
    }
}

/// `path` relative to the project, with forward slashes, when it lies inside it (links followed).
fn inside(path: &Path, project: &Path) -> Option<String> {
    let full = crate::safe::real(path);
    let root = crate::safe::real(project);
    full.strip_prefix(&root).ok().map(py::posix)
}

// ----------------------------------------------------------------------------- shell commands

/// One simple command of a shell line: its words, and the files its redirections write.
#[derive(Debug, Default, PartialEq)]
struct Simple {
    words: Vec<String>,
    writes: Vec<String>,
}

/// The state of [`simple_commands`] while it walks a shell line.
#[derive(Default)]
struct Splitter {
    out: Vec<Simple>,
    cur: Simple,
    word: String,
    has_word: bool,
    /// The next word is the target of a `>`.
    redirect: bool,
}

impl Splitter {
    fn end_word(&mut self) {
        if !self.has_word {
            return;
        }
        let word = std::mem::take(&mut self.word);
        if self.redirect {
            self.cur.writes.push(word);
        } else {
            self.cur.words.push(word);
        }
        self.has_word = false;
        self.redirect = false;
    }

    fn end_command(&mut self) {
        self.end_word();
        self.redirect = false;
        if !self.cur.words.is_empty() || !self.cur.writes.is_empty() {
            self.out.push(std::mem::take(&mut self.cur));
        }
    }
}

/// Split a shell line (POSIX shells and PowerShell, roughly) into simple commands. Quotes group,
/// `;`, `&&`, `||`, `|`, `&`, line ends and brackets separate, and `>` names a file that is
/// written. A backslash is kept as it is: on Windows it separates folders.
fn simple_commands(line: &str) -> Vec<Simple> {
    let chars: Vec<char> = line.chars().collect();
    let mut st = Splitter::default();
    let next_is = |i: usize, c: char| chars.get(i + 1) == Some(&c);
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\'' | '"' => {
                st.has_word = true;
                i += 1;
                while i < chars.len() && chars[i] != c {
                    st.word.push(chars[i]);
                    i += 1;
                }
            }
            ' ' | '\t' => st.end_word(),
            '\n' | ';' | '(' | ')' | '{' | '}' | '`' => st.end_command(),
            '|' => {
                st.end_command();
                if next_is(i, '|') {
                    i += 1;
                }
            }
            '&' if next_is(i, '>') => {
                // `&> file`: both streams to a file
                st.end_word();
                i += 1;
                if next_is(i, '>') {
                    i += 1;
                }
                st.redirect = true;
            }
            '&' => {
                st.end_command();
                if next_is(i, '&') {
                    i += 1;
                }
            }
            '>' => {
                // `2>`: the digits before it are a stream number, not a word
                if st.has_word && st.word.chars().all(|d| d.is_ascii_digit()) {
                    st.word.clear();
                    st.has_word = false;
                }
                st.end_word();
                if next_is(i, '>') || next_is(i, '|') {
                    i += 1;
                }
                if next_is(i, '&') {
                    // `>&2`, `2>&1`: a copy of a stream, no file
                    i += 1;
                    while chars
                        .get(i + 1)
                        .is_some_and(|d| d.is_ascii_digit() || *d == '-')
                    {
                        i += 1;
                    }
                } else {
                    st.redirect = true;
                }
            }
            '<' => {
                // input, or a here-document: nothing is written; its word is dropped
                st.end_word();
                while next_is(i, '<') {
                    i += 1;
                }
                while chars.get(i + 1).is_some_and(|s| *s == ' ' || *s == '\t') {
                    i += 1;
                }
                while chars
                    .get(i + 1)
                    .is_some_and(|s| !s.is_whitespace() && !matches!(s, ';' | '|' | '&' | ')'))
                {
                    i += 1;
                }
            }
            '$' if next_is(i, '(') => {
                st.end_command();
                i += 1;
            }
            _ => {
                st.word.push(c);
                st.has_word = true;
            }
        }
        i += 1;
    }
    st.end_command();
    st.out
}

/// What a shell argument names inside the project: a path with forward slashes, `.` and `..`
/// folded, seen from `base` (the folder an earlier `cd` moved to, relative to the project; `None`
/// when that folder is outside the project or unknown). `None` when the argument leaves the
/// project or is not a path this check can follow (a variable, `~`).
fn arg_path(arg: &str, base: Option<&str>, project: &Path) -> Option<String> {
    let arg = arg.replace('\\', "/");
    if arg.is_empty() || arg.contains('$') || arg.starts_with('~') {
        return None;
    }
    let drive = arg.as_bytes().get(1) == Some(&b':');
    let rel = if arg.starts_with('/') || drive {
        let same = |a: &str, b: &str| {
            if cfg!(windows) {
                a.eq_ignore_ascii_case(b)
            } else {
                a == b
            }
        };
        let slashed = |p: &Path| p.to_string_lossy().replace('\\', "/");
        let roots = [slashed(&crate::safe::real(project)), slashed(project)];
        roots.iter().find_map(|root| {
            let root = root.trim_end_matches('/');
            if same(&arg, root) {
                return Some(String::new());
            }
            let head = arg.get(..root.len() + 1)?;
            (same(&head[..root.len()], root) && head.ends_with('/'))
                .then(|| arg[root.len() + 1..].to_string())
        })?
    } else {
        match base? {
            "" => arg,
            base => format!("{base}/{arg}"),
        }
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in rel.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

/// The locked file a shell argument names: itself, one under the folder it names, or one its
/// wildcards match.
fn locked_by(
    arg: &str,
    base: Option<&str>,
    project: &Path,
    locked: &BTreeSet<String>,
) -> Option<String> {
    let rel = arg_path(arg, base, project)?;
    if rel.is_empty() {
        return locked.iter().next().cloned(); // the project folder itself
    }
    if locked.contains(&rel) {
        return Some(rel);
    }
    let under = format!("{rel}/");
    if let Some(hit) = locked.iter().find(|p| p.starts_with(&under)) {
        return Some(hit.clone());
    }
    if rel.contains(['*', '?', '[']) {
        return locked.iter().find(|p| py::fnmatch(p, &rel)).cloned();
    }
    None
}

/// The program a simple command runs (its name in lower case, without folders or `.exe`) and
/// the words after it. Leading `sudo`, `env`, `VAR=value` and the like are skipped.
fn program(cmd: &Simple) -> Option<(String, Vec<&str>)> {
    const PREFIXES: [&str; 8] = [
        "sudo", "env", "command", "time", "nohup", "exec", "xargs", "builtin",
    ];
    let mut words = cmd.words.iter().map(String::as_str).skip_while(|w| {
        PREFIXES.contains(&w.to_lowercase().as_str())
            || (w.contains('=') && !w.starts_with('-') && !w.contains('/'))
    });
    let name = words.next()?.replace('\\', "/").to_lowercase();
    let name = name.rsplit('/').next().unwrap_or("");
    let name = name.strip_suffix(".exe").unwrap_or(name).to_string();
    Some((name, words.collect()))
}

/// Does an option edit files in place (`sed -i`, `perl -pi`, `ruby -i.bak`)? `before` names the
/// short options that may share the cluster ahead of the `i`.
fn in_place(option: &str, before: &str) -> bool {
    option.starts_with("--in-place")
        || option
            .strip_prefix('-')
            .filter(|rest| !rest.starts_with('-'))
            .is_some_and(|rest| {
                let ahead: String = rest.chars().take_while(|c| *c != 'i').collect();
                ahead.len() < rest.len() && ahead.chars().all(|c| before.contains(c))
            })
}

/// The locked file a shell command would plainly remove, move or overwrite, if any. Only what can
/// be read off the command is caught here; what a script does is caught after it ran.
fn shell_target(command: &str, project: &Path, locked: &BTreeSet<String>) -> Option<String> {
    const REMOVE: [&str; 11] = [
        "rm",
        "unlink",
        "rmdir",
        "shred",
        "truncate",
        "del",
        "erase",
        "rd",
        "ri",
        "remove-item",
        "clear-content",
    ];
    const MOVE: [&str; 7] = ["mv", "move", "move-item", "mi", "rename-item", "ren", "rni"];
    const COPY: [&str; 7] = ["cp", "copy", "copy-item", "cpi", "install", "ln", "rsync"];
    const WRITE: [&str; 9] = [
        "tee",
        "set-content",
        "sc",
        "add-content",
        "ac",
        "out-file",
        "new-item",
        "ni",
        "clc",
    ];
    // where earlier `cd`s have moved, relative to the project; None once that is outside or unknown
    let mut base: Option<String> = Some(String::new());
    for cmd in simple_commands(command) {
        for target in &cmd.writes {
            if let Some(hit) = locked_by(target, base.as_deref(), project, locked) {
                return Some(hit);
            }
        }
        let Some((name, rest)) = program(&cmd) else {
            continue;
        };
        let args: Vec<&str> = rest
            .iter()
            .copied()
            .filter(|a| !a.starts_with('-'))
            .collect();
        let any = |list: &[&str]| {
            list.iter()
                .find_map(|a| locked_by(a, base.as_deref(), project, locked))
        };
        let hit = match name.as_str() {
            "cd" | "pushd" | "set-location" | "sl" | "chdir" => {
                base = args
                    .first()
                    .and_then(|to| arg_path(to, base.as_deref(), project));
                None
            }
            n if REMOVE.contains(&n) || MOVE.contains(&n) || WRITE.contains(&n) => any(&args),
            // a copy changes only where it lands
            n if COPY.contains(&n) => args.split_last().and_then(|(dest, sources)| {
                let dest = arg_path(dest, base.as_deref(), project)?;
                if locked.contains(&dest) {
                    return Some(dest);
                }
                // into a folder: the file of the same name there is overwritten
                sources.iter().find_map(|src| {
                    let src = src.replace('\\', "/");
                    let name = src.rsplit('/').next().filter(|n| !n.is_empty())?;
                    let landed = if dest.is_empty() {
                        name.to_string()
                    } else {
                        format!("{dest}/{name}")
                    };
                    locked.contains(&landed).then_some(landed)
                })
            }),
            "sed" => rest
                .iter()
                .any(|a| in_place(a, "nErsuz"))
                .then(|| any(&args))
                .flatten(),
            "perl" | "ruby" => rest
                .iter()
                .any(|a| in_place(a, "pnlae0w"))
                .then(|| any(&args))
                .flatten(),
            "dd" => rest
                .iter()
                .filter_map(|a| a.strip_prefix("of="))
                .find_map(|a| locked_by(a, base.as_deref(), project, locked)),
            "git" => match args.first().copied() {
                Some("rm") | Some("mv") => any(&args[1..]),
                _ => None,
            },
            _ => None,
        };
        if hit.is_some() {
            return hit;
        }
    }
    None
}

/// Does a shell command run `council lock-tests`?
fn relocks(command: &str) -> bool {
    simple_commands(command).iter().any(|cmd| {
        program(cmd).is_some_and(|(name, rest)| name == "council" && rest.contains(&"lock-tests"))
    })
}

// ----------------------------------------------------------------------------- locked files

/// The locked paths of a project (the manifest itself included). Empty when nothing is locked.
fn locked_set(project: &Path) -> (BTreeSet<String>, Map<String, Value>) {
    let file = project.join(".council").join("locks.json");
    let manifest = crate::safe::read_own(&file)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let Some(manifest) = manifest else {
        return (BTreeSet::new(), Map::new());
    };
    let (good, _) = crate::core::lock_entries(&manifest);
    let mut locked: BTreeSet<String> = good.keys().cloned().collect();
    if !locked.is_empty() {
        locked.insert(".council/locks.json".to_string()); // the manifest is locked too
    }
    (locked, good)
}

/// The locked path this event would change, if any.
pub fn locked_target(event: &Value, project: &Path, locked: &BTreeSet<String>) -> Option<String> {
    let input = event.at("tool_input");
    match event.s("tool_name") {
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => {
            let target = first_text(input, &["file_path", "notebook_path", "path"]);
            // locks are kept with forward slashes
            let mut names: BTreeSet<String> = [target.clone(), target.replace('\\', "/")]
                .into_iter()
                .collect();
            let full = crate::core::resolve(Path::new(&target));
            if let Ok(rel) = full.strip_prefix(crate::core::resolve(project)) {
                let rel = py::posix(rel);
                names.insert(rel.replace('\\', "/"));
                names.insert(rel);
            }
            names.into_iter().find(|n| locked.contains(n))
        }
        "Bash" | "PowerShell" => shell_target(input.s("command"), project, locked),
        _ => None,
    }
}

// ----------------------------------------------------------------------------- council seats

/// The council agent an event comes from, without a plugin's prefix (`council-loop:council-elder`).
fn seat_of(event: &Value) -> Option<&str> {
    let name = event.s("agent_type").rsplit(':').next()?;
    SEATS.contains(&name).then_some(name)
}

/// Why a council seat may not make this tool call, if it may not.
///
/// A seat's whole task is its packet, and its whole answer is one output file. The run's other
/// files hold what a blind vote must not see (who wrote what, how others voted), and the project's
/// files are not a seat's to change. Until now the packet asked for this; here it is enforced.
pub fn seat_guard(event: &Value, project: &Path) -> Option<String> {
    let seat = seat_of(event)?;
    let input = event.at("tool_input");
    let cwd = event.at("cwd").as_str().filter(|c| !c.is_empty());
    let base = cwd
        .map(PathBuf::from)
        .unwrap_or_else(|| project.to_path_buf());
    let private = |path: &Path| {
        crate::safe::within(path, &crate::safe::private_dir())
            || crate::core::user_config_path()
                .parent()
                .is_some_and(|own| crate::safe::within(path, own))
    };
    let name_of = |rel: &str| rel.rsplit('/').next().unwrap_or("").to_string();
    match event.s("tool_name") {
        "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => {
            let target = first_text(input, &["file_path", "notebook_path", "path"]);
            if target.is_empty() {
                return None;
            }
            let path = absolute(&target, &base);
            if private(&path) {
                return Some(
                    "a council seat does not write in the council's own folders".to_string(),
                );
            }
            let rel = inside(&path, project)?; // outside the project: a scratch file, not ours to judge
            let name = name_of(&rel);
            let own_output = (rel.starts_with(".council/runs/")
                || rel.starts_with(".council/map/"))
                && (name.ends_with(".out.json") || name.ends_with(".out.md"));
            (!own_output).then(|| {
                format!(
                    "a council seat writes one file: the output file its packet names. {rel} is not one. \
                     Never edit a project file; work on a copy outside the project if you need to try something."
                )
            })
        }
        "Read" => {
            let target = first_text(input, &["file_path", "path"]);
            if target.is_empty() {
                return None;
            }
            let path = absolute(&target, &base);
            if private(&path) {
                return Some(
                    "that folder holds who wrote which card: no seat opens it (the vote is blind)"
                        .to_string(),
                );
            }
            let rel = inside(&path, project);
            if let Some(rel) = rel.as_deref().filter(|r| r.starts_with(".council/runs/")) {
                let name = name_of(rel);
                let packet = name.ends_with(".prompt.md")
                    || name.ends_with(".retry.md")
                    || name == "facts.md"
                    || name == "skeleton.md";
                return (!packet).then(|| {
                    "inside the run's folders a seat opens only its own packet: the other files hold what a \
                     blind vote must not see"
                        .to_string()
                });
            }
            (seat == "council-researcher").then(|| {
                "a researcher reads the web and its own packet, nothing else: a seat that can open web pages \
                 does not also read files (a page could talk it into sending one out)"
                    .to_string()
            })
        }
        "Grep" | "Glob" => {
            if seat == "council-researcher" {
                return Some(
                    "a researcher does not search the project's files: it reads the web and its own packet"
                        .to_string(),
                );
            }
            let target = first_text(input, &["path"]);
            let path = if target.is_empty() {
                base.clone()
            } else {
                absolute(&target, &base)
            };
            let in_runs = inside(&path, project)
                .is_some_and(|rel| rel == ".council/runs" || rel.starts_with(".council/runs/"));
            (private(&path) || in_runs).then(|| {
                "a seat does not search the run's folders: they hold what a blind vote must not see"
                    .to_string()
            })
        }
        _ => None,
    }
}

// ----------------------------------------------------------------------------- the two events

fn deny(reason: &str) -> String {
    Json::LINE.dumps(
        &json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny",
                                        "permissionDecisionReason": reason}}),
    )
}

fn allow_locked() -> bool {
    std::env::var("COUNCIL_ALLOW_LOCKED").is_ok_and(|v| v == "1")
}

/// Before a tool runs: the JSON line that denies it, or `None` to let it pass.
fn before(event: &Value, project: &Path) -> Option<String> {
    if let Some(why) = seat_guard(event, project) {
        return Some(deny(&why));
    }
    if allow_locked() {
        return None;
    }
    let (locked, _) = locked_set(project);
    if locked.is_empty() {
        return None;
    }
    if let Some(hit) = locked_target(event, project, &locked) {
        return Some(deny(&format!(
            "{hit} is locked by the Council Loop (tests/answer key). Fix the code, not the test. Ask the requester to unlock if the test is wrong."
        )));
    }
    let shell = matches!(event.s("tool_name"), "Bash" | "PowerShell");
    if shell && relocks(event.at("tool_input").s("command")) {
        return Some(deny(
            "council lock-tests would replace the locks that are in force. Only the requester re-locks, after \
             approving the test change (they set COUNCIL_ALLOW_LOCKED=1 for the session). Ask them.",
        ));
    }
    None
}

/// Where the hook remembers what it has checked and what it has already said, per project.
fn state_file(project: &Path) -> Option<PathBuf> {
    let own = crate::core::user_config_path().parent()?.to_path_buf();
    let key = crate::core::sha256_hex(crate::safe::real(project).to_string_lossy().as_bytes());
    Some(own.join("cache").join(format!("locks-{}.json", &key[..16])))
}

/// Size and time of change of a plain file as one comparable text, and whether that change is so
/// recent that another one could carry the same time (file systems keep time in steps of up to
/// two seconds).
fn stamp(path: &Path) -> Option<(String, bool)> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.file_type().is_file() || meta.len() > crate::safe::MAX_LOCKED {
        return None;
    }
    let changed = meta.modified().ok()?;
    let recent = changed
        .elapsed()
        .map_or(true, |age| age < std::time::Duration::from_secs(3));
    let nanos = changed
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some((format!("{}:{nanos}", meta.len()), recent))
}

/// The locked files that do not match their lock now, and of those the ones not yet reported.
/// A file is hashed only when its size or time of change differs from the last look.
pub fn drifted(project: &Path, state: &mut Value) -> (Vec<String>, Vec<String>) {
    let (_, good) = locked_set(project);
    let mut seen = state.at("seen").as_object().cloned().unwrap_or_default();
    let mut told = state.at("told").as_object().cloned().unwrap_or_default();
    let (mut off, mut fresh) = (Vec::new(), Vec::new());
    let mut next_seen = Map::new();
    for (path, digest) in &good {
        let file = project.join(path);
        let (now, recent) = stamp(&file).unwrap_or_else(|| ("missing".to_string(), false));
        let before = seen.remove(path);
        let ok = match before
            .as_ref()
            .filter(|b| b.s("stamp") == now && b.at("lock") == digest)
        {
            Some(b) => b.truthy("ok"),
            None => {
                now != "missing"
                    && crate::safe::within(&file, project)
                    && crate::core::sha_file(&file).ok().as_deref() == digest.as_str()
            }
        };
        // a file changed moments ago is looked at again next time: its stamp is not trusted yet
        let kept = if recent { String::new() } else { now.clone() };
        next_seen.insert(
            path.clone(),
            json!({"stamp": kept, "lock": digest, "ok": ok}),
        );
        if ok {
            told.remove(path);
        } else {
            off.push(path.clone());
            if told.get(path).and_then(Value::as_str) != Some(now.as_str()) {
                fresh.push(path.clone());
                told.insert(path.clone(), json!(now));
            }
        }
    }
    told.retain(|path, _| good.contains_key(path));
    *state = json!({"seen": next_seen, "told": told});
    (off, fresh)
}

/// After a shell command ran: the JSON line that tells the model a locked file no longer matches
/// its lock, or `None`.
fn after(_event: &Value, project: &Path) -> Option<String> {
    if allow_locked() || !project.join(".council").join("locks.json").exists() {
        return None;
    }
    let file = state_file(project)?;
    let mut state: Value = crate::safe::read_capped(&file, crate::safe::MAX_OUTPUT * 8)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| json!({}));
    let before = state.clone();
    let (_, fresh) = drifted(project, &mut state);
    if state != before {
        // replaced in one step: two hooks of one session may finish together
        let tmp = file.with_extension(format!("{}.tmp", std::process::id()));
        if crate::safe::write_text(&tmp, &Json::LINE.dumps(&state)).is_ok() {
            let _ = crate::safe::rename(&tmp, &file);
        }
    }
    if fresh.is_empty() {
        return None;
    }
    let shown: Vec<&str> = fresh.iter().take(12).map(String::as_str).collect();
    let more = if fresh.len() > shown.len() {
        format!(" and {} more", fresh.len() - shown.len())
    } else {
        String::new()
    };
    let reason = format!(
        "Locked file(s) no longer match their lock: {}{more}. Locked files are the answer key: fix the code, not \
         the test. If the command you just ran changed them, undo that change now (for a file git tracks: \
         git checkout -- <file>). If the requester changed them on purpose, do not touch them: ask the requester \
         to lock them again (council lock-tests <paths>).",
        shown.join(", ")
    );
    Some(Json::LINE.dumps(&json!({"decision": "block", "reason": reason})))
}

/// The answer to one hook event: the JSON line that objects to it, or `None` to let it pass.
pub fn decide(raw: &[u8]) -> Option<String> {
    // bytes, not the locale's idea of text (Windows); a byte-order mark is tolerated
    let text = String::from_utf8_lossy(raw.strip_prefix(b"\xef\xbb\xbf").unwrap_or(raw));
    let event: Value = serde_json::from_str(&text).ok().filter(Value::is_object)?;
    let project = std::env::var("CLAUDE_PROJECT_DIR")
        .ok()
        .filter(|p| !p.is_empty())
        .or_else(|| {
            event
                .at("cwd")
                .as_str()
                .filter(|c| !c.is_empty())
                .map(str::to_string)
        });
    let project = PathBuf::from(project.unwrap_or_else(|| ".".to_string()));
    let verdict = match event.s("hook_event_name") {
        "PostToolUse" => after(&event, &project),
        "" | "PreToolUse" => before(&event, &project),
        _ => None,
    };
    crate::diag!(Debug, "hook", "{} {}: {}", event.s("hook_event_name"), event.s("tool_name"),
                 if verdict.is_some() { "objected" } else { "passed" }; "agent" => event.s("agent_type"));
    verdict
}

/// `council hook`: read one event on standard input and answer it.
pub fn run() -> i32 {
    let mut raw = Vec::new();
    if std::io::stdin()
        .take(MAX_EVENT)
        .read_to_end(&mut raw)
        .is_ok()
    {
        if let Some(objection) = decide(&raw) {
            crate::say!("{objection}");
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locked() -> BTreeSet<String> {
        [
            "tests/test_x.py",
            "tests/unit/test_y.py",
            ".council/locks.json",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect()
    }

    #[test]
    fn edits_of_locked_files_are_found_by_any_spelling_of_the_path() {
        let project = std::env::temp_dir().join("council-hook-test");
        let edit = |path: String| json!({"tool_name": "Edit", "tool_input": {"file_path": path}});
        let hit = |event: Value| locked_target(&event, &project, &locked());
        assert_eq!(
            hit(edit("tests/test_x.py".into())).as_deref(),
            Some("tests/test_x.py")
        );
        assert_eq!(
            hit(edit("tests\\test_x.py".into())).as_deref(),
            Some("tests/test_x.py")
        );
        assert_eq!(
            hit(edit(
                project
                    .join("tests")
                    .join("test_x.py")
                    .display()
                    .to_string()
            ))
            .as_deref(),
            Some("tests/test_x.py")
        );
        assert_eq!(hit(edit("src/app.py".into())), None);
        assert_eq!(hit(json!({"tool_name": "NotebookEdit", "tool_input": {"notebook_path": ".council/locks.json"}})).as_deref(), Some(".council/locks.json"));
        assert_eq!(
            hit(json!({"tool_name": "Read", "tool_input": {"file_path": "tests/test_x.py"}})),
            None
        );
    }

    #[test]
    fn a_shell_line_is_split_into_commands_words_and_written_files() {
        let words = |s: &[&str]| s.iter().map(|w| (*w).to_string()).collect::<Vec<_>>();
        assert_eq!(
            simple_commands(
                "cd tests && rm -f 'a b.py' x.py; echo done > out.log 2>&1 | tee -a log"
            ),
            vec![
                Simple {
                    words: words(&["cd", "tests"]),
                    writes: vec![]
                },
                Simple {
                    words: words(&["rm", "-f", "a b.py", "x.py"]),
                    writes: vec![]
                },
                Simple {
                    words: words(&["echo", "done"]),
                    writes: words(&["out.log"])
                },
                Simple {
                    words: words(&["tee", "-a", "log"]),
                    writes: vec![]
                },
            ]
        );
        assert_eq!(
            simple_commands("pytest -q tests 2>&1 >/dev/null &> all.txt"),
            vec![Simple {
                words: words(&["pytest", "-q", "tests"]),
                writes: words(&["/dev/null", "all.txt"])
            }]
        );
        assert_eq!(
            simple_commands("cat <<'EOF' > notes.md\nhello\nEOF"),
            vec![
                Simple {
                    words: words(&["cat"]),
                    writes: words(&["notes.md"])
                },
                Simple {
                    words: words(&["hello"]),
                    writes: vec![]
                },
                Simple {
                    words: words(&["EOF"]),
                    writes: vec![]
                },
            ]
        );
        assert!(simple_commands("").is_empty() && simple_commands(" ;; | ").is_empty());
    }

    #[test]
    fn shell_commands_are_denied_only_when_they_plainly_change_a_locked_file() {
        let project = PathBuf::from("/work/app");
        let bash = |command: &str| shell_target(command, &project, &locked());
        let x = Some("tests/test_x.py");
        // removed, moved, overwritten, edited in place
        for command in [
            "sed -i 's/1/2/' tests/test_x.py",
            "sed -i.bak -e 's/1/2/' ./tests/test_x.py",
            "perl -pi -e 's/1/2/' tests/test_x.py",
            "echo x > tests/test_x.py",
            "echo x >> tests/test_x.py",
            "cat new.py >tests/test_x.py",
            "Remove-Item tests\\test_x.py",
            "rm tests/test_x.py",
            "rm -f /work/app/tests/test_x.py",
            "cd tests && rm test_x.py",
            "cd src; cd ../tests; rm test_x.py",
            "rm tests/test_*.py",
            "mv tests/test_x.py tests/old.py",
            "cp /tmp/new.py tests/test_x.py",
            "cp /tmp/test_x.py tests/",
            "git rm tests/test_x.py",
            "python gen.py | tee tests/test_x.py",
            "dd if=/dev/zero of=tests/test_x.py",
            "sudo rm tests/test_x.py",
            "FOO=1 rm tests/test_x.py",
            "true && (rm tests/test_x.py)",
            "Set-Content -Path tests\\test_x.py -Value x",
        ] {
            assert_eq!(bash(command).as_deref(), x, "{command}");
        }
        // a whole folder of locked files
        assert!(bash("rm -rf tests").is_some());
        assert!(bash("rm -rf tests/unit").is_some());
        assert!(bash("git mv tests old-tests").is_some());
        assert!(bash("rm -rf .").is_some());
        // reading, running and writing elsewhere pass (this is what agents do all day)
        for command in [
            "cat tests/test_x.py",
            "pytest -q tests/test_x.py",
            "pytest -q tests/test_x.py 2>&1 | tail -5",
            "pytest tests > /tmp/out.log 2>&1",
            "python -m pytest tests/test_x.py | tee /tmp/log.txt",
            "rm src/app.py",
            "cp tests/test_x.py /tmp/copy.py",
            "cp /tmp/other.py tests/",
            "sed -n '1,5p' tests/test_x.py",
            "sed 's/a/b/' tests/test_x.py > /tmp/changed.py",
            "git checkout -- tests/test_x.py",
            "git diff tests/test_x.py",
            "grep -rn 'rm tests/test_x.py' docs",
            "touch tests/test_x.py",
            "rm $FILE",
            "cd /elsewhere && rm tests/test_x.py",
            "rm ../outside/tests/test_x.py",
        ] {
            assert_eq!(bash(command), None, "{command}");
        }
    }

    #[test]
    fn locking_again_is_noticed() {
        assert!(relocks("council lock-tests tests"));
        assert!(relocks(
            "cd app && ./target/release/council lock-tests tests/ migrations/"
        ));
        assert!(relocks(
            "COUNCIL_ALLOW_LOCKED=1 council.exe lock-tests tests"
        ));
        assert!(!relocks("council verify-locks"));
        assert!(!relocks("echo run council lock-tests later > notes.txt"));
        assert!(!relocks("grep -rn lock-tests docs"));
    }

    #[test]
    fn seats_stay_in_their_lane() {
        let project = std::env::temp_dir().join(format!("council-seat-{}", std::process::id()));
        let run = project
            .join(".council")
            .join("runs")
            .join("r1")
            .join("loop-1");
        std::fs::create_dir_all(run.join("ideate")).unwrap();
        std::fs::create_dir_all(project.join("src")).unwrap();
        let call = |agent: &str, tool: &str, key: &str, path: &Path| {
            seat_guard(
                &json!({"agent_type": agent, "tool_name": tool, "cwd": project.display().to_string(),
                        "tool_input": {key: path.display().to_string()}}),
                &project,
            )
        };
        let ideate = run.join("ideate");
        // its own output file and its own packet: fine, with or without a plugin's prefix
        assert_eq!(
            call(
                "council-expert",
                "Write",
                "file_path",
                &ideate.join("L1.out.json")
            ),
            None
        );
        assert_eq!(
            call(
                "council-loop:council-mapper",
                "Write",
                "file_path",
                &run.join("research").join("MAP.out.md")
            ),
            None
        );
        assert_eq!(
            call(
                "council-expert",
                "Read",
                "file_path",
                &ideate.join("L1.prompt.md")
            ),
            None
        );
        assert_eq!(
            call(
                "council-expert",
                "Read",
                "file_path",
                &ideate.join("L1.retry.md")
            ),
            None
        );
        assert_eq!(
            call(
                "council-expert",
                "Read",
                "file_path",
                &project.join("src").join("app.py")
            ),
            None
        );
        // a scratch file outside the project is not this hook's business
        assert_eq!(
            call(
                "council-red-team",
                "Write",
                "file_path",
                &std::env::temp_dir().join("scratch-x").join("t.py")
            ),
            None
        );
        // not a project file, not the run's state, not another seat's output
        assert!(call(
            "council-expert",
            "Write",
            "file_path",
            &project.join("src").join("app.py")
        )
        .is_some());
        assert!(call(
            "council-expert",
            "Edit",
            "file_path",
            &project
                .join(".council")
                .join("runs")
                .join("r1")
                .join("run.json")
        )
        .is_some());
        assert!(call(
            "council-expert",
            "Write",
            "file_path",
            &ideate.join("L1.prompt.md")
        )
        .is_some());
        assert!(call(
            "council-loop:council-elder",
            "Read",
            "file_path",
            &ideate.join("L2.out.json")
        )
        .is_some());
        assert!(call(
            "council-expert",
            "Read",
            "file_path",
            &project
                .join(".council")
                .join("runs")
                .join("r1")
                .join("run.json")
        )
        .is_some());
        assert!(call(
            "council-expert",
            "Grep",
            "path",
            &project.join(".council").join("runs")
        )
        .is_some());
        assert_eq!(
            call("council-expert", "Grep", "path", &project.join("src")),
            None
        );
        // the researcher reads the web and its packet
        assert_eq!(
            call(
                "council-researcher",
                "Read",
                "file_path",
                &run.join("research").join("RA.prompt.md")
            ),
            None
        );
        assert!(call(
            "council-researcher",
            "Read",
            "file_path",
            &project.join("src").join("app.py")
        )
        .is_some());
        assert!(call("council-researcher", "Glob", "path", &project).is_some());
        // anyone else is not a seat: the chair, the user's own agents
        assert_eq!(
            call(
                "",
                "Write",
                "file_path",
                &project.join("src").join("app.py")
            ),
            None
        );
        assert_eq!(
            call(
                "code-reviewer",
                "Read",
                "file_path",
                &ideate.join("L2.out.json")
            ),
            None
        );
        assert_eq!(
            call(
                "council-something-else",
                "Write",
                "file_path",
                &project.join("x")
            ),
            None
        );
        let _ = std::fs::remove_dir_all(&project);
    }

    #[test]
    fn a_changed_locked_file_is_reported_once_and_hashed_only_when_it_changed() {
        let project = std::env::temp_dir().join(format!("council-drift-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&project);
        std::fs::create_dir_all(project.join("tests")).unwrap();
        std::fs::create_dir_all(project.join(".council")).unwrap();
        std::fs::write(project.join("tests").join("a.py"), "assert 1 == 1\n").unwrap();
        std::fs::write(project.join("tests").join("b.py"), "assert 2 == 2\n").unwrap();
        let lock = |name: &str| crate::core::sha_file(&project.join("tests").join(name)).unwrap();
        let manifest = json!({"tests/a.py": lock("a.py"), "tests/b.py": lock("b.py")});
        std::fs::write(
            project.join(".council").join("locks.json"),
            manifest.to_string(),
        )
        .unwrap();
        let mut state = json!({});
        assert_eq!(drifted(&project, &mut state), (vec![], vec![]));
        assert_eq!(state["seen"]["tests/a.py"]["ok"], true);
        // one file changes: reported once, then remembered
        std::fs::write(
            project.join("tests").join("a.py"),
            "assert 1 == 2  # made to pass\n",
        )
        .unwrap();
        let both = (
            vec!["tests/a.py".to_string()],
            vec!["tests/a.py".to_string()],
        );
        assert_eq!(drifted(&project, &mut state), both);
        assert_eq!(
            drifted(&project, &mut state),
            (vec!["tests/a.py".to_string()], vec![])
        );
        // put back: quiet again, and a later change is news again
        std::fs::write(project.join("tests").join("a.py"), "assert 1 == 1\n").unwrap();
        assert_eq!(drifted(&project, &mut state), (vec![], vec![]));
        std::fs::remove_file(project.join("tests").join("b.py")).unwrap();
        assert_eq!(
            drifted(&project, &mut state).1,
            vec!["tests/b.py".to_string()]
        );
        // a file changed moments ago is hashed again next time (its time stamp is not trusted yet)
        assert_eq!(state["seen"]["tests/a.py"]["stamp"], "");
        // an old, unchanged file is not read again: a verdict put in the cache by hand stays
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
        let a = std::fs::File::options()
            .write(true)
            .open(project.join("tests").join("a.py"))
            .unwrap();
        a.set_modified(old).unwrap();
        drop(a);
        assert_eq!(
            drifted(&project, &mut state).0,
            vec!["tests/b.py".to_string()]
        );
        assert_ne!(state["seen"]["tests/a.py"]["stamp"], "");
        state["seen"]["tests/a.py"]["ok"] = json!(false);
        assert_eq!(drifted(&project, &mut state).0.len(), 2);
        let _ = std::fs::remove_dir_all(&project);
    }
}
