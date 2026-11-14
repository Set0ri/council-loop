//! `council hook`: the Claude Code PreToolUse hook that blocks edits to locked files.
//!
//! ELI5: the answer key is locked in a drawer. The AI can read it but not change it.
//!
//! Files are locked with `council lock-tests`. Unlock: the project owner sets
//! `COUNCIL_ALLOW_LOCKED=1` for the session (needed to run `council lock-tests` again after an
//! approved test change), or the change goes through a PR labelled council-unlock. The hook is a
//! convenience and can be bypassed; CI running `council verify-locks --against origin/<base>` is
//! the real lock.
//!
//! The hook reads the event as JSON on standard input. To deny, it prints one JSON object; to
//! allow, it prints nothing. It always exits 0: a hook that fails must never block the session.

use crate::py::{self, Json};
use crate::val::Get;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Shell words that write, move or delete (POSIX shells, cmd.exe and PowerShell).
static WRITES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(>|>>|\btee\b|\bsed\s+-i|\brm\b|\bmv\b|\bcp\b|\btruncate\b|\bgit\s+(checkout|restore)\b|\bdel\b|\berase\b|\b(Remove|Move|Copy|Rename)-Item\b|\b(Set|Add|Clear)-Content\b|\bOut-File\b)",
    )
    .expect("valid")
});

/// The first truthy text among some fields of the tool input.
fn first_text(input: &Value, keys: &[&str]) -> String {
    keys.iter()
        .map(|k| input.at(k))
        .find(|v| py::truthy(v))
        .map(py::to_str)
        .unwrap_or_default()
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
        "Bash" => {
            let command = input.s("command").replace('\\', "/");
            if !WRITES.is_match(&command) {
                return None;
            }
            locked
                .iter()
                .find(|p| command.contains(p.as_str()))
                .cloned()
        }
        _ => None,
    }
}

/// The answer to one hook event: the JSON line that denies it, or `None` to let it pass.
pub fn decide(raw: &[u8]) -> Option<String> {
    if std::env::var("COUNCIL_ALLOW_LOCKED").is_ok_and(|v| v == "1") {
        return None;
    }
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
    let manifest: Value = serde_json::from_str(
        &crate::core::read_text(&project.join(".council").join("locks.json")).ok()?,
    )
    .ok()?;
    let mut locked: BTreeSet<String> = match &manifest {
        Value::Object(map) => map.keys().cloned().collect(),
        Value::Array(list) => list
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        _ => return None,
    };
    locked.insert(".council/locks.json".to_string()); // the manifest is locked too
    let hit = locked_target(&event, &project, &locked)?;
    let reason = format!("{hit} is locked by the Council Loop (tests/answer key). Fix the code, not the test. Ask the requester to unlock if the test is wrong.");
    Some(Json::LINE.dumps(
        &json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny",
                                                         "permissionDecisionReason": reason}}),
    ))
}

/// `council hook`: read one event on standard input and answer it.
pub fn run() -> i32 {
    let mut raw = Vec::new();
    if std::io::stdin().read_to_end(&mut raw).is_ok() {
        if let Some(denial) = decide(&raw) {
            crate::say!("{denial}");
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locked() -> BTreeSet<String> {
        ["tests/test_x.py", ".council/locks.json"]
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
    fn shell_commands_are_denied_only_when_they_write() {
        let project = PathBuf::from(".");
        let bash = |command: &str| {
            locked_target(
                &json!({"tool_name": "Bash", "tool_input": {"command": command}}),
                &project,
                &locked(),
            )
        };
        assert_eq!(
            bash("sed -i 's/1/2/' tests/test_x.py").as_deref(),
            Some("tests/test_x.py")
        );
        assert_eq!(
            bash("echo x > tests/test_x.py").as_deref(),
            Some("tests/test_x.py")
        );
        assert_eq!(
            bash("Remove-Item tests\\test_x.py").as_deref(),
            Some("tests/test_x.py")
        );
        assert_eq!(
            bash("git checkout -- tests/test_x.py").as_deref(),
            Some("tests/test_x.py")
        );
        assert_eq!(bash("cat tests/test_x.py"), None);
        assert_eq!(bash("rm src/app.py"), None);
        assert_eq!(bash("pytest -q tests/test_x.py"), None);
    }
}
