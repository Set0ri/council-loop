//! The first run on a machine: a quick look around, said once.
//!
//! ELI5: the first time you pick up a new tool, it tells you in ten lines whether it can work
//! here, what it will cost by default, and the fastest way to see it do something. After that it
//! stays quiet.
//!
//! The check is local and fast (a few file lookups, well under a tenth of a second): which
//! command-line tools are on the PATH, which keys are set, how old the model data is. Nothing is
//! sent anywhere and no model is called. It is printed on standard error, so a command's own
//! output stays clean, and a marker in your settings folder (`~/.council/install.json`) records
//! that it was shown. `council doctor` prints the full picture any time.

use crate::py::Json;
use crate::val::Get;
use serde_json::{json, Value};
use std::path::PathBuf;

/// Where the marker lives: in your settings folder.
fn marker() -> Option<PathBuf> {
    crate::core::user_config_path()
        .parent()
        .map(|own| own.join("install.json"))
}

/// Is this a place where nobody reads a welcome (a CI job), or was it switched off?
fn silenced() -> bool {
    let on = |name: &str| {
        std::env::var(name)
            .is_ok_and(|v| !matches!(v.to_ascii_lowercase().as_str(), "" | "0" | "false" | "no"))
    };
    on("COUNCIL_NO_WELCOME") || on("CI")
}

/// The lines of the first-run check, from what [`crate::gate::doctor`] found.
pub fn lines(env: &Value) -> Vec<String> {
    let mut out = vec![format!(
        "council {} · first run on this machine (a quick local check: nothing was sent anywhere)",
        crate::assets::VERSION
    )];
    let mark = |ok: bool| if ok { "[ok]" } else { "[--]" };
    let fresh = env.s("fresh_contexts");
    out.push(match fresh {
        "subagents" => "  [ok] seats can start: as subagents of this Claude Code session".to_string(),
        "none" => "  [--] seats cannot start in fresh contexts yet: install the claude, codex or gemini CLI (or run inside Claude Code)".to_string(),
        cli => format!("  [ok] seats can start: through the {} CLI", cli.trim_start_matches("cli:")),
    });
    let labs: Vec<&str> = crate::val::items(env.at("labs_reachable"))
        .filter(|(_, ok)| crate::py::truthy(ok))
        .map(|(name, _)| name.as_str())
        .collect();
    out.push(match labs.len() {
        0 => "  [--] no model lab reachable from here yet".to_string(),
        1 => format!(
            "  [ok] lab reachable: {} (a second lab adds an independent review: install codex or gemini, or set OPENROUTER_API_KEY)",
            labs[0]
        ),
        _ => format!("  [ok] labs reachable: {}", labs.join(", ")),
    });
    let lock = env.at("lock");
    out.push(if !lock.truthy("exists") || lock.has("error") {
        "  [--] model data is missing or unreadable: council refresh".to_string()
    } else {
        format!(
            "  {} model data: {} day(s) old{}",
            mark(!lock.truthy("stale")),
            lock.show("age_days"),
            if lock.truthy("stale") {
                " (stale: council refresh)"
            } else {
                ""
            }
        )
    });
    out.push(format!(
        "  {} git: {}",
        mark(env.truthy("git")),
        if env.truthy("git") {
            "found"
        } else {
            "not found (the code map then walks the folders itself)"
        }
    ));
    let memory = env.at("memory");
    out.push(format!(
        "  {} shared memory: {}",
        mark(memory.truthy("ok")),
        if memory.s("backend") == "obsidian" {
            "your Obsidian vault"
        } else if memory.s("source") == "default" {
            "plain files in each project's .council/memory"
        } else {
            "plain files, where your settings say"
        }
    ));
    let mode = env.s("recommended_mode");
    let cost = env
        .at("mode_costs")
        .get(mode)
        .map(|c| format!("; rel. cost {}", crate::py::repr(c)))
        .unwrap_or_default();
    out.push(format!(
        "  a run here defaults to mode={mode} profile={} (the cheapest that works{cost})",
        crate::gate::DEFAULT_PROFILE
    ));
    out.push("  see a whole council now, with no model and at no cost:  council demo".to_string());
    out.push("  start a real one:                                       council init \"<the ask, in the requester's own words>\"".to_string());
    out.push(
        "  this check again, in full:                              council doctor".to_string(),
    );
    out
}

/// Say the first-run check once per machine (and one line after an update). `verb` is the command
/// being run: the hook, the MCP server and the API speak to programs, and stay silent.
pub fn first_run(verb: &str) {
    if matches!(verb, "hook" | "mcp" | "api" | "doctor" | "demo") || silenced() {
        return;
    }
    let Some(path) = marker() else { return };
    let seen = crate::core::read_text(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let version = crate::assets::VERSION;
    match &seen {
        Some(mark) if mark.s("version") == version => return,
        Some(mark) if mark.truthy("version") => {
            crate::complain!(
                "council was updated: {} -> {version}. What changed: CHANGELOG.md (or `council doctor` for a fresh look around)",
                mark.s("version")
            );
        }
        _ => {
            let started = std::time::Instant::now();
            let env = crate::gate::doctor(None, false);
            for line in lines(&env) {
                crate::complain!("{line}");
            }
            crate::diag!(
                Debug,
                "welcome",
                "the first-run check took {} ms",
                started.elapsed().as_millis()
            );
        }
    }
    let first = seen
        .as_ref()
        .map(|mark| mark.show("first_run"))
        .filter(|date| !date.is_empty())
        .unwrap_or_else(crate::clock::today_iso);
    let mark = json!({"version": version, "first_run": first, "note": "written by council after its first-run check; delete it to see the check again"});
    // a settings folder that cannot be written is not worth a word: the check is simply shown again
    let _ = crate::safe::write_text(&path, &format!("{}\n", Json::ONE.dumps(&mark)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_check_says_what_works_what_it_costs_and_what_to_do_next() {
        let env = json!({"fresh_contexts": "subagents", "labs_reachable": {"Anthropic": true, "OpenAI": false, "Google": false},
                         "lock": {"exists": true, "age_days": 1, "stale": false}, "git": true,
                         "memory": {"backend": "files", "source": "default", "ok": true},
                         "recommended_mode": "fanout:claude", "mode_costs": {"fanout:claude": 0.66}});
        let text = lines(&env).join("\n");
        assert!(
            text.contains("[ok] seats can start: as subagents"),
            "{text}"
        );
        assert!(
            text.contains("[ok] lab reachable: Anthropic (a second lab"),
            "{text}"
        );
        assert!(text.contains("[ok] model data: 1 day(s) old"), "{text}");
        assert!(
            text.contains(
                "mode=fanout:claude profile=efficient (the cheapest that works; rel. cost 0.66)"
            ),
            "{text}"
        );
        assert!(
            text.contains("council demo")
                && text.contains("council init")
                && text.contains("council doctor")
        );
        let bare = json!({"fresh_contexts": "none", "labs_reachable": {}, "lock": {"exists": false}, "git": false,
                          "memory": {"backend": "obsidian", "ok": false}, "recommended_mode": "solo", "mode_costs": {}});
        let text = lines(&bare).join("\n");
        assert!(
            text.contains("[--] seats cannot start")
                && text.contains("[--] no model lab reachable"),
            "{text}"
        );
        assert!(
            text.contains("[--] model data is missing") && text.contains("[--] git: not found"),
            "{text}"
        );
        assert!(
            text.contains("mode=solo profile=efficient (the cheapest that works)"),
            "{text}"
        );
    }
}
