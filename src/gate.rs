//! The gate: what can be called from this machine, and how big a council the ask deserves.
//!
//! `doctor` looks around (command-line tools, keys, the age of the model data) and recommends a mode.
//! `plan` sizes the work without spending a token: the chair's judgment picks a tier, tripwires can
//! only raise it, and only the requester's own words can lower it again.

use crate::core::{self, MemberFlags};
use crate::py;
use crate::val::Get;
use serde_json::{json, Value};
use std::path::Path;

/// The gate's inputs, apart from the ask itself.
#[derive(Clone, Debug)]
pub struct GateArgs {
    /// `auto`, `direct`, `lite` or `full`.
    pub tier: String,
    /// `solo`, `crosslab` or `fanout:<family>`; `None` = your settings' default, else the cheapest
    /// mode that can be reached from here.
    pub mode: Option<String>,
    /// `max`, `balanced` or `efficient`; `None` = your settings' default, else `efficient`.
    pub profile: Option<String>,
    /// The loop cap, when the requester set one.
    pub loops: Option<i64>,
    /// Files the task will touch.
    pub paths: Vec<String>,
    /// The chair's judgment: `low`, `medium` or `high`.
    pub stakes: Option<String>,
    /// The chair's judgment: `yes` or `no`.
    pub reversible: Option<String>,
    /// The requester's own words, when they override the gate.
    pub override_words: Option<String>,
    /// Settings for the researchers and the mapper.
    pub flags: MemberFlags,
}

impl Default for GateArgs {
    fn default() -> Self {
        GateArgs {
            tier: "auto".to_string(),
            mode: None,
            profile: None,
            loops: None,
            paths: Vec::new(),
            stakes: None,
            reversible: None,
            override_words: None,
            flags: MemberFlags::default(),
        }
    }
}

fn on_path(name: &str) -> bool {
    which::which(name).is_ok()
}

/// Every mode that can be chosen, in the order ties are broken (a second lab's check first).
pub const MODES: [&str; 7] = [
    "crosslab",
    "fanout:claude",
    "fanout:openai",
    "fanout:gemini",
    "fanout:openrouter",
    "fanout:mix",
    "fanout:open",
];

/// The modes that can be reached from here (see [`doctor`]) with their relative cost for a profile,
/// cheapest first. A mode the model data has no cost for is left out.
pub fn mode_costs(env: &Value, profile: &str) -> Vec<(&'static str, f64)> {
    let lab = |name: &str| env.at("labs_reachable").truthy(name);
    let labs = ["Anthropic", "OpenAI", "Google"]
        .iter()
        .filter(|l| lab(l))
        .count();
    let mut out: Vec<(&'static str, f64)> = MODES
        .iter()
        .filter(|mode| match **mode {
            "crosslab" => labs >= 2,
            "fanout:claude" => lab("Anthropic"),
            "fanout:openai" => lab("OpenAI"),
            "fanout:gemini" => lab("Google"),
            _ => env.truthy("openrouter_key"),
        })
        .filter_map(|mode| {
            core::models_for(mode, profile)
                .at("relative_cost")
                .as_f64()
                .map(|cost| (*mode, cost))
        })
        .collect();
    out.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)); // stable: ties keep MODES order
    out
}

/// The defaults you set for yourself in `~/.council/config.yaml` (`defaults: {mode, profile}`),
/// and what was wrong with them. A project's settings file cannot set these: it travels with the
/// repository, and what a run costs is yours to decide.
pub fn own_defaults() -> (Option<String>, Option<String>, Vec<String>) {
    let path = core::user_config_path();
    let own = if path.exists() {
        core::load_yaml(&path).unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    let own = own.at("defaults");
    let (mut mode, mut profile, mut problems) = (None, None, Vec::new());
    if let Some(m) = own.at("mode").as_str() {
        let family = m.strip_prefix("fanout:").unwrap_or("");
        if m == "solo" || m == "crosslab" || core::FAMILIES.contains(&family) {
            mode = Some(m.to_string());
        } else {
            problems.push(format!(
                "defaults.mode in {} is not a mode (solo, crosslab or fanout:<{}>): ignored",
                path.display(),
                core::FAMILIES.join("|")
            ));
        }
    }
    if let Some(p) = own.at("profile").as_str() {
        if core::PROFILES.contains(&p) {
            profile = Some(p.to_string());
        } else {
            problems.push(format!(
                "defaults.profile in {} is not a profile ({}): ignored",
                path.display(),
                core::PROFILES.join(", ")
            ));
        }
    }
    (mode, profile, problems)
}

/// The profile a run uses when nobody chose one: the cheapest.
pub const DEFAULT_PROFILE: &str = "efficient";

/// What can be called from here? Recommends a mode. Spends no tokens unless `probe` is set, which
/// sends one tiny test prompt to each installed CLI (an installed CLI may still be logged out).
pub fn doctor(root: Option<&Path>, probe: bool) -> Value {
    let env = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    let cli = [
        ("claude", on_path("claude")),
        ("codex", on_path("codex")),
        ("gemini", on_path("gemini")),
    ];
    let has = |name: &str| cli.iter().any(|(n, ok)| *n == name && *ok);
    let claude_code = env("CLAUDECODE").as_deref() == Some("1");
    let openrouter = env("OPENROUTER_API_KEY").is_some();
    let labs = [
        ("Anthropic", claude_code || has("claude")),
        ("OpenAI", has("codex") || openrouter),
        ("Google", has("gemini") || openrouter),
    ];
    let lab = |name: &str| labs.iter().any(|(n, ok)| *n == name && *ok);
    let lock_text = crate::assets::read_model("lock.yaml");
    let mut lock = json!({"exists": lock_text.is_some(), "age_days": null, "stale": null});
    if let Some(text) = &lock_text {
        let found = crate::yaml::load(text)
            .map_err(|e| e.to_string())
            .and_then(|data| {
                let collected = data.at("data").show("collected");
                let limit = core::policy()
                    .at("data")
                    .int("stale_after_days")
                    .unwrap_or(30);
                let date = crate::clock::parse_date(&collected).ok_or_else(|| {
                    format!("Invalid isoformat string: {}", py::str_repr(&collected))
                })?;
                Ok::<_, String>(((crate::clock::today() - date).num_days(), limit, collected))
            });
        match found {
            Ok((age, limit, collected)) => {
                lock["age_days"] = json!(age);
                lock["stale"] = json!(age > limit);
                lock["collected"] = json!(collected);
            }
            Err(err) => lock["error"] = json!(err),
        }
    }
    let reachable = labs.iter().filter(|(_, ok)| *ok).count();
    // the most independent review that can be reached: another lab's models where there is one
    let mode = if reachable >= 2 {
        "crosslab"
    } else if lab("Anthropic") {
        "fanout:claude"
    } else if lab("OpenAI") {
        "fanout:openai"
    } else if lab("Google") {
        "fanout:gemini"
    } else {
        "solo"
    };
    let mut notes: Vec<String> = Vec::new();
    let missing: Vec<&str> = labs
        .iter()
        .filter(|(_, ok)| !*ok)
        .map(|(n, _)| *n)
        .collect();
    if mode == "crosslab" && !missing.is_empty() {
        notes.push(format!(
            "{} not reachable: those seats become labelled stand-ins",
            missing.join(", ")
        ));
    }
    if mode.starts_with("fanout:") || mode == "solo" {
        notes.push(
            "only one lab reachable: elders come from the same family (no cross-lab check). \
             Install the codex / gemini CLIs or set OPENROUTER_API_KEY to get one"
                .to_string(),
        );
    }
    if lock.truthy("stale") {
        notes.push(format!(
            "model data is {} days old: run `council refresh`",
            lock.show("age_days")
        ));
    }
    if !lock.truthy("exists") {
        notes.push(
            "models/lock.yaml missing: run `council refresh --offline <snapshot>`".to_string(),
        );
    }
    let mut fresh = if claude_code {
        "subagents".to_string()
    } else {
        cli.iter()
            .find(|(_, ok)| *ok)
            .map(|(n, _)| format!("cli:{n}"))
            .unwrap_or_else(|| "none".to_string())
    };
    if let Some(forced) = env("COUNCIL_SEATS_VIA") {
        fresh = forced; // subagents | cli:claude | cli:codex | cli:gemini | none
    }
    let mut probes = serde_json::Map::new();
    let mut notes_probe: Vec<String> = Vec::new();
    if probe {
        for (name, provider) in [
            ("claude", "claude"),
            ("codex", "openai"),
            ("gemini", "gemini"),
        ] {
            if !has(name) {
                continue;
            }
            let res = crate::providers::ask(
                provider,
                "Reply with exactly the two letters: OK",
                None,
                None,
                90,
                false,
                "none",
            );
            let said = res.show("text").to_uppercase();
            let ok = res.truthy("ok") && said.contains("OK") && !said.contains("ERROR");
            let outcome = if ok {
                "ok".to_string()
            } else {
                let detail = if res.truthy("text") {
                    res.show("text")
                } else if res.truthy("error") {
                    res.show("error")
                } else {
                    String::new()
                };
                format!("FAILED: {}", py::head(&py::squash(&detail), 120))
            };
            if !ok {
                notes_probe.push(format!(
                    "the {name} CLI is installed but did not answer a test prompt: {outcome}"
                ));
            }
            probes.insert(name.to_string(), json!(outcome));
        }
    }
    if fresh == "none" {
        notes.push("no way to start fresh contexts: seats will run in the chair's context (logged as a deviation)".to_string());
    }
    notes.extend(notes_probe);
    let mem = crate::memory::settings(root);
    if !mem.truthy("ok") {
        notes.push(mem.show("problem"));
    }
    let mut out = serde_json::Map::new();
    out.insert("claude_code".into(), json!(claude_code));
    out.insert(
        "cli".into(),
        cli.iter()
            .map(|(n, ok)| ((*n).to_string(), json!(ok)))
            .collect::<serde_json::Map<_, _>>()
            .into(),
    );
    if probe {
        out.insert("probes".into(), Value::Object(probes));
    }
    out.insert("git".into(), json!(on_path("git")));
    out.insert("openrouter_key".into(), json!(openrouter));
    out.insert("aa_key".into(), json!(env("AA_API_KEY").is_some()));
    out.insert(
        "labs_reachable".into(),
        labs.iter()
            .map(|(n, ok)| ((*n).to_string(), json!(ok)))
            .collect::<serde_json::Map<_, _>>()
            .into(),
    );
    out.insert("fresh_contexts".into(), json!(fresh));
    out.insert("lock".into(), lock);
    // the default is the cheapest mode that can be reached; the strongest is one flag away
    let reach = Value::Object(out.clone());
    let costs = mode_costs(&reach, DEFAULT_PROFILE);
    let cheapest = costs.first().map_or(mode, |(m, _)| *m);
    if cheapest != mode {
        let cost_of = |m: &str| {
            costs
                .iter()
                .find(|(name, _)| *name == m)
                .map(|(_, c)| format!(" (rel. cost {})", py::repr(&json!(c))))
                .unwrap_or_default()
        };
        notes.push(format!(
            "the default mode is the cheapest that can be reached: {cheapest}{}. For another lab's check of the finalists: --mode {mode}{}",
            cost_of(cheapest),
            cost_of(mode)
        ));
    }
    out.insert("recommended_mode".into(), json!(cheapest));
    out.insert("strongest_mode".into(), json!(mode));
    out.insert(
        "mode_costs".into(),
        costs
            .iter()
            .map(|(m, c)| ((*m).to_string(), json!(c)))
            .collect::<serde_json::Map<_, _>>()
            .into(),
    );
    out.insert("notes".into(), json!(notes));
    out.insert(
        "council".into(),
        json!(format!("{} (rust)", crate::assets::VERSION)),
    );
    out.insert("memory".into(), json!({"backend": mem.at("backend"), "dir": mem.at("dir"), "source": mem.at("source"), "ok": mem.at("ok")}));
    Value::Object(out)
}

/// Dry run of the gate. Spends no model tokens. Tripwires only raise the tier; only the requester's
/// own words (`override_words`) can lower it. The chair records its judgment with stakes / reversible.
pub fn plan(task: &str, a: &GateArgs) -> Value {
    let root = core::project_root();
    let hits = core::tripwires_hit(task, &a.paths, &root);
    let (stakes, reversible) = (a.stakes.as_deref(), a.reversible.as_deref());
    let (mut chosen, mut reason): (&str, String) = if core::TIERS.contains(&a.tier.as_str()) {
        (a.tier.as_str(), "explicit".to_string())
    } else if stakes == Some("high") || reversible == Some("no") {
        (
            "full",
            format!(
                "chair: {}",
                if stakes == Some("high") {
                    "stakes=high"
                } else {
                    "not reversible"
                }
            ),
        )
    } else if stakes == Some("low") {
        ("direct", "chair: stakes=low, reversible".to_string())
    } else if stakes == Some("medium") {
        ("lite", "chair: stakes=medium".to_string())
    } else {
        (
            "lite",
            "default: pass --stakes and --reversible to record your judgment".to_string(),
        )
    };
    let words = a.override_words.as_deref().filter(|w| !w.is_empty());
    if !hits.is_empty() && chosen != "full" {
        match words {
            Some(words) => {
                reason = format!(
                    "tripwire(s) {} waived by the requester: \"{words}\"",
                    hits.join(", ")
                )
            }
            None => {
                chosen = "full";
                reason = format!("raised by tripwire(s): {}", hits.join(", "));
            }
        }
    } else if let Some(words) = words {
        reason = format!("the requester: \"{words}\"");
    }
    let env = doctor(None, false);
    // who chose the mode and the profile: a flag, your own settings, or nobody (then the cheapest)
    let (own_mode, own_profile, own_problems) = own_defaults();
    let given = |flag: &Option<String>| flag.clone().filter(|v| !v.is_empty());
    let (profile, profile_from) = match (given(&a.profile), own_profile) {
        (Some(p), _) => (p, "flag"),
        (None, Some(p)) => (p, "your settings"),
        (None, None) => (DEFAULT_PROFILE.to_string(), "default"),
    };
    let (mode, mode_from) = match (given(&a.mode), own_mode) {
        (Some(m), _) => (m, "flag"),
        (None, Some(m)) => (m, "your settings"),
        (None, None) => (
            mode_costs(&env, &profile).first().map_or_else(
                || env.s("recommended_mode").to_string(),
                |(m, _)| (*m).to_string(),
            ),
            "default",
        ),
    };
    let mut shape = json!({});
    let mut n_loops = 0usize;
    if let Some(s) = core::shape(chosen) {
        n_loops = a
            .loops
            .filter(|n| *n > 0)
            .map(|n| n as usize)
            .unwrap_or(s.loops);
        shape = s.to_value();
        shape["loops"] = json!(n_loops);
        shape["schedule"] = json!(core::schedule_for(s.start, n_loops));
    }
    let picks = core::models_for(&mode, &profile);
    let mut support = core::members(&root, &a.flags);
    if core::shape(chosen).is_none() {
        // the researchers and the mapper sit with a council only
        support["research"]["seats"] = json!(0);
        support["map"]["when"] = json!("never");
    }
    support["map"]["due"] = json!(false);
    support["map"]["reason"] = json!("switched off");
    if support["map"].s("when") != "never" {
        let st = crate::codemap::stale(&root, &crate::codemap::scan(&root));
        let always = support["map"].s("when") == "always" && st.truthy("applies");
        support["map"]["due"] = json!(always || (st.truthy("applies") && st.truthy("stale")));
        support["map"]["reason"] = if always && !st.truthy("stale") {
            json!("set to run every time")
        } else {
            st.at("reason").clone()
        };
    }
    let n_res = support["research"].int("seats").unwrap_or(0).max(0) as usize;
    let due = support["map"].truthy("due");
    let calls = core::estimate_calls(chosen, n_loops, n_res, due);
    let tokens = core::estimate_tokens(chosen, n_loops, &core::policy(), &profile, n_res, due);
    let shown_mode = picks
        .get("mode")
        .map(py::to_str)
        .unwrap_or_else(|| mode.clone());
    let gate = format!(
        "GATE tier={chosen} mode={shown_mode} profile={profile} loops<={n_loops} calls~{calls} tokens~{} rel_cost={} ({reason})",
        core::short_tokens(tokens),
        picks.get("relative_cost").map(py::to_str).unwrap_or_else(|| "n/a".to_string())
    );
    crate::diag!(Info, "gate", "tier={chosen} ({reason}); mode={shown_mode} ({mode_from}); profile={profile} ({profile_from})";
                 "tripwires" => hits.clone(), "calls" => calls, "tokens" => tokens);
    let mut notes: Vec<Value> = env.list("notes").to_vec();
    notes.extend(own_problems.into_iter().map(Value::String));
    if core::shape(chosen).is_some() && (mode_from == "default" || profile_from == "default") {
        let what = match (mode_from, profile_from) {
            ("default", "default") => format!("mode={shown_mode} and profile={profile} are the cheapest that work here (the default)"),
            ("default", _) => format!("mode={shown_mode} is the cheapest that works here for profile={profile} (the default)"),
            _ => format!("profile={profile} is the cheapest (the default)"),
        };
        let second_lab = if env.s("strongest_mode") == "crosslab" && shown_mode != "crosslab" {
            ", --mode crosslab"
        } else {
            ""
        };
        notes.push(json!(format!(
            "{what}. More rigour costs more: --profile balanced or max{second_lab}. Keep a choice: `defaults:` in your ~/.council/config.yaml"
        )));
    }
    json!({"gate": gate, "tier": chosen, "reason": reason, "tripwires": hits, "mode": shown_mode, "profile": profile,
           "chosen_by": {"mode": mode_from, "profile": profile_from},
           "shape": shape, "calls_estimate": calls, "tokens_estimate": tokens, "models": picks, "support": support,
           "environment_notes": notes})
}

/// One line under the GATE line: who else sits at the table, and where the shared memory is.
pub fn members_line(support: &Value, tier: &str, memory_conf: Option<&Value>) -> String {
    if core::shape(tier).is_none() {
        return "MEMBERS none (direct tier: no council)".to_string();
    }
    let beats = core::beats_for(support);
    let res = if beats.is_empty() {
        "researchers=0 (off)".to_string()
    } else {
        format!(
            "researchers={} ({}; about {} searches each)",
            beats.len(),
            beats
                .iter()
                .map(|b| b.0.to_lowercase())
                .collect::<Vec<_>>()
                .join("; "),
            support.at("research").at("budget").show(tier)
        )
    };
    let m = support.at("map");
    let mapper = if m.s("when") == "never" {
        "mapper=off".to_string()
    } else {
        format!(
            "mapper={} ({})",
            if m.truthy("due") { "yes" } else { "not needed" },
            m.show("reason")
        )
    };
    let own;
    let conf = match memory_conf {
        Some(conf) => conf,
        None => {
            own = crate::memory::settings(None);
            &own
        }
    };
    let location = if conf.s("backend") == "obsidian" {
        conf.show("folder")
    } else {
        conf.show("dir")
    };
    let root = core::project_root().display().to_string();
    let prefix = format!("{root}{}", std::path::MAIN_SEPARATOR);
    // inside the project: shown from its root, with forward slashes, so the line reads the same on every system
    let location = match location.strip_prefix(&prefix) {
        Some(inside) => inside.replace(std::path::MAIN_SEPARATOR, "/"),
        None => location,
    };
    format!(
        "MEMBERS {res} {mapper} memory={}:{location}",
        conf.show("backend")
    )
}
