//! `council mcp`: serve the skill and the council tools to any MCP client over stdio.
//!
//! ELI5: an MCP server is a plug. Any AI app that speaks MCP (Claude Code, Claude Desktop, Cursor,
//! Codex, Gemini CLI, ...) can plug in and get (1) the Council Loop instructions and (2) the
//! bookkeeping tools, so every app runs the same process the same way.
//!
//! It is a thin wrapper: every tool calls the function behind the CLI verb of the same name, so
//! there is one enforcement path for the command line, the plugin hook, CI and MCP.
//!
//! The run-loop tools (`council_init`, `council_next`, `council_ingest`, ...) take `project_dir`,
//! the absolute path of the project the run belongs to, because a server process does not share the
//! client's working directory. They return the text the CLI would print, which is the chair's exact
//! next step.
//!
//! The transport is JSON-RPC 2.0, one message per line on standard input and output. Nothing but
//! protocol messages is ever written to standard output.

use crate::core::{self, MemberFlags, ResearchFlag, UserError};
use crate::gate::{self, GateArgs};
use crate::py::{self, Json};
use crate::val::Get;
use crate::{codemap, memory, providers, run, say};
use anyhow::Result;
use serde_json::{json, Map, Value};
use std::io::{BufRead as _, Write as _};

/// Protocol revisions this server answers. A client that asks for another one is offered the newest.
const VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "The Council Loop: a gated, blind, checked decision process. Read the resource \
skill://council-loop/SKILL.md first. The run-loop tools are the `council` verbs of the same name (council_init, then \
council_next and council_ingest in a loop, council_cut at the end of each loop, council_status after a context reset). \
They take project_dir, the absolute path of the project, and return the text the command line would print: the exact next step.";

// ----------------------------------------------------------------------------- tool table

#[derive(Clone, Copy)]
enum Kind {
    Str,
    Int,
    Bool,
    StrList,
    Obj,
    ObjList,
}

/// One argument of a tool. `default = None` means the argument is required.
struct Param {
    name: &'static str,
    kind: Kind,
    default: Option<Value>,
}

fn req(name: &'static str, kind: Kind) -> Param {
    Param {
        name,
        kind,
        default: None,
    }
}

fn opt(name: &'static str, kind: Kind, default: Value) -> Param {
    Param {
        name,
        kind,
        default: Some(default),
    }
}

/// An optional argument that is left out by default (`null`).
fn nul(name: &'static str, kind: Kind) -> Param {
    opt(name, kind, Value::Null)
}

struct Tool {
    name: &'static str,
    about: &'static str,
    params: Vec<Param>,
    call: fn(&Value) -> Result<Value>,
}

fn tools() -> Vec<Tool> {
    use Kind::{Bool, Int, Obj, ObjList, Str, StrList};
    let project = || req("project_dir", Str);
    let run_id = || nul("run_id", Str);
    vec![
        Tool {
            name: "council_doctor",
            about: "What can be called from the machine this server runs on (CLIs, keys, model-data age) and which\n\
                    mode fits. probe=True sends one tiny test prompt to each installed CLI (an installed CLI may be\n\
                    logged out).",
            params: vec![opt("probe", Bool, json!(false))],
            call: |a| Ok(gate::doctor(None, flag(a, "probe"))),
        },
        Tool {
            name: "council_init",
            about: "Start a run in project_dir. `ask` is the requester's own words, verbatim. Returns the GATE line,\n\
                    the MEMBERS line and the first step. stakes: low|medium|high. reversible: yes|no. override: the\n\
                    requester's own words when they set the tier themselves.\n\
                    Support members (leave unset to use the project's config): research = how many researchers, 0 to 4;\n\
                    research_budget = searches per researcher; research_later = on_request|always|never; mapper =\n\
                    stale|always|never (when the codebase map is rewritten); focus_tokens = size of the ranked code list\n\
                    in packets. obsidian=True keeps the run's shared memory in the user's Obsidian vault (set once with\n\
                    council_memory_setup).",
            params: vec![
                req("ask", Str),
                project(),
                opt("tier", Str, json!("auto")),
                nul("mode", Str),
                opt("profile", Str, json!("balanced")),
                nul("loops", Int),
                nul("stakes", Str),
                nul("reversible", Str),
                nul("override", Str),
                nul("paths", StrList),
                nul("research", Int),
                nul("mapper", Str),
                opt("obsidian", Bool, json!(false)),
                nul("research_budget", Int),
                nul("research_later", Str),
                nul("focus_tokens", Int),
            ],
            call: |a| {
                let args = gate_args(a, text(a, "mode").map(str::to_string), text(a, "mapper").map(str::to_string));
                in_project(a, || run::cmd_init(a.s("ask"), &args, flag(a, "obsidian")))
            },
        },
        Tool {
            name: "council_next",
            about: "Prepare the current step (writes the seats' prompt files) and return exactly what to do now.\n\
                    Safe to call again: after a restart it lists only the seats that still owe an output.",
            params: vec![project(), run_id(), nul("without_control", Str)],
            call: |a| in_project(a, || run::cmd_next(text(a, "run_id"), text(a, "without_control"))),
        },
        Tool {
            name: "council_ingest",
            about: "Check what came back for the current step; advance when it is all valid, else say what is owed.\n\
                    drop: seat ids to go on without (e.g. \"L3\" or \"EB,EC\"), with a reason. Never drops a seat you did not name.",
            params: vec![project(), run_id(), nul("drop", Str), nul("reason", Str)],
            call: |a| in_project(a, || run::cmd_ingest(text(a, "run_id"), text(a, "drop"), text(a, "reason"))),
        },
        Tool {
            name: "council_dispatch",
            about: "Run the seats of the current step that live behind a CLI or API (codex, gemini, claude -p,\n\
                    OpenRouter). Sends prompts to those vendors. dry_run=True only shows the commands.",
            params: vec![project(), run_id(), nul("seat", Str), opt("dry_run", Bool, json!(false))],
            call: |a| in_project(a, || run::cmd_dispatch(text(a, "run_id"), text(a, "seat"), flag(a, "dry_run"))),
        },
        Tool {
            name: "council_cut",
            about: "End the loop: apply the vote, checks and elder verdicts, write the ledger, start the next loop or\n\
                    the final. eli5: one line on what happened. At a judgment point pass asked (\"question -> answer\") or\n\
                    unattended (the safer option taken).",
            params: vec![project(), req("eli5", Str), opt("changed", Str, json!("")), nul("stop", Str), nul("asked", Str), nul("unattended", Str), run_id()],
            call: |a| {
                in_project(a, || run::cmd_cut(text(a, "run_id"), a.s("eli5"), a.s("changed"), text(a, "stop"), text(a, "asked"), text(a, "unattended")))
            },
        },
        Tool {
            name: "council_status",
            about: "One-screen digest of the run and what the current step still owes. Use it after a context reset.",
            params: vec![project(), run_id()],
            call: |a| in_project(a, || run::cmd_status(text(a, "run_id"))),
        },
        Tool {
            name: "council_deviation",
            about: "Record that the run left the protocol, and why. It reaches the ledger, the final and the retro.",
            params: vec![project(), req("text", Str), run_id()],
            call: |a| in_project(a, || run::cmd_deviation(text(a, "run_id"), a.s("text"))),
        },
        Tool {
            name: "council_audit",
            about: "Re-count every vote of a run from the ballots on disk and compare with the record.",
            params: vec![project(), run_id()],
            call: |a| in_project(a, || run::cmd_audit(text(a, "run_id"))),
        },
        Tool {
            name: "council_note",
            about: "Post to the current run's commons as the chair. to: \"all\" (members read it unsigned at the next\n\
                    ideation or red-team step), \"research\" (a question the researchers take up before the next loop)\n\
                    or \"chair\" (kept for the record).",
            params: vec![project(), req("text", Str), opt("to", Str, json!("all")), run_id()],
            call: |a| in_project(a, || run::cmd_note(text(a, "run_id"), a.s("text"), text(a, "to").unwrap_or("all"))),
        },
        Tool {
            name: "council_memory_search",
            about: "Find facts in the project's research memory by keyword. Each fact has a source and a confidence.",
            params: vec![project(), req("query", Str), nul("tag", Str), opt("limit", Int, json!(10))],
            call: |a| in_project(a, || memory::cmd_search(a.s("query"), text(a, "tag"), whole(a, "limit").unwrap_or(10), false)),
        },
        Tool {
            name: "council_memory_add",
            about: "Add one fact to the research memory by hand. source: a web address you opened, a file in the\n\
                    project, or \"test: <command>\". A fact without a source is refused.",
            params: vec![
                project(),
                req("claim", Str),
                req("source", Str),
                opt("quote", Str, json!("")),
                opt("kind", Str, json!("other")),
                opt("confidence", Str, json!("medium")),
                opt("tags", Str, json!("")),
                opt("date", Str, json!("")),
                opt("why", Str, json!("")),
            ],
            call: |a| {
                let or = |key: &str, fallback: &'static str| text(a, key).unwrap_or(fallback).to_string();
                let (kind, confidence) = (or("kind", "other"), or("confidence", "medium"));
                in_project(a, || memory::cmd_add(a.s("claim"), a.s("source"), a.s("quote"), &kind, &confidence, a.s("tags"), a.s("date"), a.s("why"), "chair"))
            },
        },
        Tool {
            name: "council_memory_status",
            about: "Where the shared memory lives (project files or an Obsidian vault) and what it holds.",
            params: vec![project()],
            call: |a| in_project(a, memory::cmd_status),
        },
        Tool {
            name: "council_memory_setup",
            about: "Choose where the shared memory lives. obsidian_vault: the path of the user's Obsidian vault on the\n\
                    machine this server runs on (saved in the user's own settings unless for_project). Without it:\n\
                    plain files, in files_dir or the project's .council/memory.",
            params: vec![project(), nul("obsidian_vault", Str), nul("folder", Str), nul("files_dir", Str), opt("for_project", Bool, json!(false))],
            call: |a| {
                let vault = text(a, "obsidian_vault");
                in_project(a, || memory::cmd_setup(vault.is_some(), vault, text(a, "folder"), vault.is_none(), text(a, "files_dir"), flag(a, "for_project")))
            },
        },
        Tool {
            name: "council_map",
            about: "The codebase map (docs/ARCHITECTURE.md). action: \"skeleton\" writes the script-made skeleton and\n\
                    says whether the map is behind the code; \"check\" fails when it is; \"validate\" checks the written\n\
                    map (sections, and that every path it names exists); \"packet\" writes the mapper's packet so one\n\
                    fresh agent can refresh the map; \"install\" validates `file` and puts it in place; \"accept\" records\n\
                    that the map describes the code as it is now.",
            params: vec![project(), opt("action", Str, json!("skeleton")), nul("file", Str)],
            call: |a| {
                let action = text(a, "action").unwrap_or("skeleton");
                if action == "packet" {
                    return in_project(a, run::cmd_map_packet);
                }
                if !matches!(action, "skeleton" | "check" | "validate" | "accept" | "install") {
                    return Ok(json!({"ok": false, "output": "action must be skeleton, check, validate, packet, install or accept"}));
                }
                let file = text(a, "file");
                if action == "install" && file.is_none() {
                    return Ok(json!({"ok": false, "output": "install needs `file`: the Markdown document to install"}));
                }
                let args = codemap::MapArgs {
                    check: action == "check",
                    accept: action == "accept",
                    validate: action == "validate",
                    budget: 420,
                    install: file.filter(|_| action == "install").map(str::to_string),
                    ..Default::default()
                };
                in_project(a, || codemap::cmd_map(&args))
            },
        },
        Tool {
            name: "council_plan",
            about: "Dry-run the gate: tier (tripwires can only raise it), models per role, cut schedule, the support\n\
                    members (research = how many researchers, 0 to 4; mapper = stale|always|never) and the estimated\n\
                    seat calls and tokens. Spends no model tokens. Pass project_dir to use that project's config and to\n\
                    count the mapper when its map is missing or behind its code.",
            params: vec![
                req("task", Str),
                opt("tier", Str, json!("auto")),
                opt("mode", Str, json!("crosslab")),
                opt("profile", Str, json!("balanced")),
                nul("loops", Int),
                nul("paths", StrList),
                nul("project_dir", Str),
                nul("research", Int),
                nul("mapper", Str),
                nul("research_budget", Int),
            ],
            call: |a| {
                let mode = Some(text(a, "mode").unwrap_or("crosslab").to_string());
                let root = text(a, "project_dir").map(py::path).filter(|root| root.is_absolute() && root.is_dir());
                let Some(root) = root else {
                    return Ok(gate::plan(a.s("task"), &gate_args(a, mode, Some("never".to_string()))));
                };
                let args = gate_args(a, mode, text(a, "mapper").map(str::to_string));
                let before = std::env::current_dir().ok();
                std::env::set_current_dir(&root)?;
                let plan = gate::plan(a.s("task"), &args);
                if let Some(before) = before {
                    let _ = std::env::set_current_dir(before);
                }
                Ok(plan)
            },
        },
        Tool {
            name: "council_config",
            about: "Every setting for the researchers, the mapper and the shared memory in this project, and who set\n\
                    it (a flag, the project's .council/config.yaml, the user's ~/.council/config.yaml, or the default).",
            params: vec![project()],
            call: |a| {
                in_project(a, || {
                    let root = core::project_root();
                    let settings = core::members(&root, &MemberFlags::default());
                    say!("{}", core::members_report(&settings, &memory::settings(Some(&root)), &root).join("\n"));
                    Ok(0)
                })
            },
        },
        Tool {
            name: "council_models",
            about: "Model picks per role for a mode (solo, crosslab, fanout:claude|gemini|openai|openrouter|open|mix)\n\
                    and profile (max, balanced, efficient).",
            params: vec![opt("mode", Str, json!("crosslab")), opt("profile", Str, json!("balanced"))],
            call: |a| Ok(core::models_for(text(a, "mode").unwrap_or("crosslab"), text(a, "profile").unwrap_or("balanced"))),
        },
        Tool {
            name: "council_blind",
            about: "Anonymise + shuffle candidate cards ({id, authors, title, body, eli5}) into a judge packet, and\n\
                    refuse cards that say who wrote them. Keep the returned mapping away from judges.",
            params: vec![req("cards", ObjList), opt("seed", Int, json!(0)), opt("max_words", Int, json!(120))],
            call: |a| {
                let seed = whole(a, "seed").unwrap_or(0).max(0) as u64;
                let max_words = whole(a, "max_words").unwrap_or(120).max(0) as usize;
                Ok(core::blind(a.list("cards"), seed, max_words, &[], 1.5))
            },
        },
        Tool {
            name: "council_tally",
            about: "Count blind ballots as head-to-heads weighted by each voter's stake in the cards (a voter's own\n\
                    card earns nothing from that voter). Failed checks rank last. Reports `thin` pairs the vote could not\n\
                    judge. Optional cut to `keep` with one Pareto wildcard.",
            params: vec![req("votes", ObjList), req("mapping", Obj), nul("scores", Obj), nul("checks", Obj), nul("keep", Int)],
            call: |a| {
                let given = |key: &str| a.get(key).filter(|v| !v.is_null());
                let keep = whole(a, "keep").map(|k| k.max(0) as usize);
                let opt = core::TallyOptions { scores: given("scores"), checks: given("checks"), keep, ..Default::default() };
                Ok(core::tally(a.list("votes"), a.at("mapping"), &opt))
            },
        },
        Tool {
            name: "council_check_run",
            about: "Validate a run ledger against the protocol (idea counts, caps, ELI5s, loops, red team, elders).",
            params: vec![req("run", Obj)],
            call: |a| Ok(core::check_run(a.at("run"))),
        },
        Tool {
            name: "council_verify_locks",
            about: "Report locked files (tests, migrations, ...) that changed. CI should run the same check.",
            params: vec![],
            call: |_| core::verify_locks(&core::project_root(), None),
        },
        Tool {
            name: "council_elder_review",
            about: "Send one blinded packet to the cross-lab elders (claude, openai, gemini, openrouter) in parallel.\n\
                    Defaults to dry_run=True: set False to actually call the CLIs/APIs.",
            params: vec![req("packet", Str), opt("profile", Str, json!("balanced")), nul("providers", StrList), opt("dry_run", Bool, json!(true))],
            call: |a| {
                let names = crate::val::strings(a.at("providers"));
                let dry_run = a.get("dry_run").and_then(Value::as_bool).unwrap_or(true);
                Ok(providers::elder_review(a.s("packet"), text(a, "profile").unwrap_or("balanced"), &names, None, dry_run))
            },
        },
    ]
}

// ----------------------------------------------------------------------------- arguments

/// A text argument that was given (and is not `null`).
fn text<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn flag(args: &Value, key: &str) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn whole(args: &Value, key: &str) -> Option<i64> {
    args.get(key).and_then(crate::val::as_int)
}

/// The gate's inputs from a tool call (`council_init` and `council_plan` share them).
fn gate_args(a: &Value, mode: Option<String>, mapper: Option<String>) -> GateArgs {
    GateArgs {
        tier: text(a, "tier").unwrap_or("auto").to_string(),
        mode,
        profile: text(a, "profile").unwrap_or("balanced").to_string(),
        loops: whole(a, "loops"),
        paths: crate::val::strings(a.at("paths")),
        stakes: text(a, "stakes").map(str::to_string),
        reversible: text(a, "reversible").map(str::to_string),
        override_words: text(a, "override").map(str::to_string),
        flags: MemberFlags {
            research: whole(a, "research").map(ResearchFlag::Seats),
            mapper,
            research_budget: whole(a, "research_budget"),
            research_later: text(a, "research_later").map(str::to_string),
            focus_tokens: whole(a, "focus_tokens"),
        },
    }
}

/// Run one CLI verb inside a project and return what it would have printed.
fn in_project(args: &Value, work: impl FnOnce() -> Result<i32>) -> Result<Value> {
    let given = args.s("project_dir");
    let root = py::path(given);
    if !root.is_absolute() || !root.is_dir() {
        return Ok(
            json!({"ok": false, "output": format!("project_dir must be an existing absolute directory (got {})", py::str_repr(given))}),
        );
    }
    let before = std::env::current_dir().ok();
    std::env::set_current_dir(&root)?;
    let (result, printed) = crate::out::capture(work);
    if let Some(before) = before {
        let _ = std::env::set_current_dir(before);
    }
    match result {
        Ok(code) => Ok(json!({"ok": code == 0, "output": py::strip(&printed)})),
        // not a crash: the tool says what is still owed
        Err(err) if err.downcast_ref::<UserError>().is_some() => {
            Ok(json!({"ok": false, "output": py::strip(&format!("{printed}\nerror: {err}"))}))
        }
        Err(err) => Err(err),
    }
}

/// The JSON Schema of one argument, spelled the way the Python SDK spells it.
fn schema_of(p: &Param) -> Value {
    let kind = match p.kind {
        Kind::Str => json!({"type": "string"}),
        Kind::Int => json!({"type": "integer"}),
        Kind::Bool => json!({"type": "boolean"}),
        Kind::StrList => json!({"items": {"type": "string"}, "type": "array"}),
        Kind::Obj => json!({"additionalProperties": true, "type": "object"}),
        Kind::ObjList => {
            json!({"items": {"additionalProperties": true, "type": "object"}, "type": "array"})
        }
    };
    let title: Vec<String> = p
        .name
        .split('_')
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|first| first.to_uppercase().chain(c).collect())
                .unwrap_or_default()
        })
        .collect();
    let mut out = match &p.default {
        Some(Value::Null) => json!({"anyOf": [kind, {"type": "null"}], "default": null}),
        Some(default) => {
            let mut out = json!({"default": default});
            for (k, v) in crate::val::items(&kind) {
                out[k.as_str()] = v.clone();
            }
            out
        }
        None => kind,
    };
    out["title"] = json!(title.join(" "));
    out
}

fn describe(tool: &Tool) -> Value {
    let properties: Map<String, Value> = tool
        .params
        .iter()
        .map(|p| (p.name.to_string(), schema_of(p)))
        .collect();
    let required: Vec<&str> = tool
        .params
        .iter()
        .filter(|p| p.default.is_none())
        .map(|p| p.name)
        .collect();
    let mut schema = json!({"type": "object", "properties": properties, "title": format!("{}Arguments", tool.name)});
    if !required.is_empty() {
        schema["required"] = json!(required);
    }
    json!({"name": tool.name, "description": tool.about, "inputSchema": schema})
}

/// What is wrong with the arguments of a call, if anything: a missing one, or one of the wrong type.
fn problem_with(tool: &Tool, args: &Value) -> Option<String> {
    for p in &tool.params {
        let value = args
            .get(p.name)
            .filter(|v| !(v.is_null() && p.default.is_some()));
        let Some(value) = value else {
            if p.default.is_none() {
                return Some(format!("{}: this argument is required", p.name));
            }
            continue;
        };
        let (fits, wanted) = match p.kind {
            Kind::Str => (value.is_string(), "text"),
            Kind::Int => (crate::val::as_int(value).is_some(), "a whole number"),
            Kind::Bool => (value.is_boolean(), "true or false"),
            Kind::StrList => (
                value
                    .as_array()
                    .is_some_and(|a| a.iter().all(Value::is_string)),
                "a list of texts",
            ),
            Kind::Obj => (value.is_object(), "an object"),
            Kind::ObjList => (
                value
                    .as_array()
                    .is_some_and(|a| a.iter().all(Value::is_object)),
                "a list of objects",
            ),
        };
        if !fits {
            return Some(format!("{}: must be {wanted}", p.name));
        }
    }
    None
}

// ----------------------------------------------------------------------------- resources and the prompt

const SKILL_URI: &str = "skill://council-loop/SKILL.md";
const LOCK_URI: &str = "council://models/lock";

fn or_missing(text: Option<String>, name: &str) -> String {
    text.unwrap_or_else(|| format!("(missing: {name})"))
}

/// The name a client put in a resource address, with `%xx` read back. `None` when it is not one
/// plain file name: a resource never leaves its folder.
fn leaf(part: &str) -> Option<String> {
    let bytes = part.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = (bytes[i] == b'%')
            .then(|| {
                part.get(i + 1..i + 3)
                    .and_then(|h| u8::from_str_radix(h, 16).ok())
            })
            .flatten();
        match hex {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    let name = String::from_utf8(out).ok()?;
    let plain =
        !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', '\0']);
    plain.then_some(name)
}

fn read_resource(uri: &str) -> Option<String> {
    if uri == SKILL_URI {
        return Some(or_missing(crate::assets::read("SKILL.md"), "SKILL.md"));
    }
    if uri == LOCK_URI {
        return Some(or_missing(
            crate::assets::read_model("lock.yaml"),
            "lock.yaml",
        ));
    }
    if let Some(name) = uri.strip_prefix("council://reference/") {
        let safe = leaf(name)?;
        let safe = safe.strip_suffix(".md").unwrap_or(&safe);
        return Some(or_missing(
            crate::assets::read(&format!("reference/{safe}.md")),
            &format!("{safe}.md"),
        ));
    }
    if let Some(name) = uri.strip_prefix("council://templates/") {
        let safe = leaf(name)?;
        let found = [
            format!("templates/{safe}"),
            format!("templates/{safe}.md"),
            format!("templates/packets/{safe}"),
            format!("templates/packets/{safe}.md"),
        ]
        .iter()
        .find_map(|rel| crate::assets::read(rel));
        return Some(or_missing(found, &safe));
    }
    None
}

const PROMPT_ABOUT: &str = "Run the Council Loop on a task. tier: auto|direct|lite|full. mode: solo|crosslab|fanout:<family>.\n\
                            profile: max|balanced|efficient.";

fn prompt_text(args: &Value) -> String {
    let task = args.s("task");
    let gate_args = GateArgs {
        tier: text(args, "tier").unwrap_or("auto").to_string(),
        mode: Some(text(args, "mode").unwrap_or("crosslab").to_string()),
        profile: text(args, "profile").unwrap_or("balanced").to_string(),
        ..Default::default()
    };
    let plan = gate::plan(task, &gate_args);
    format!(
        "{}\n\n---\nTASK (the requester's words, verbatim): {task}\n{}\n\
         Start by showing the GATE line, then follow the skill for that tier. Without a shell, use this \
         server's tools in place of the commands: council_init, then council_next and council_ingest in a \
         loop, council_cut at the end of each loop, council_status after a context reset.",
        or_missing(crate::assets::read("SKILL.md"), "SKILL.md"),
        plan.s("gate")
    )
}

// ----------------------------------------------------------------------------- the protocol

/// A JSON-RPC error: (code, message).
type Refusal = (i64, String);

fn call_tool(params: &Value) -> Result<Value, Refusal> {
    let name = params.s("name");
    let all = tools();
    let Some(tool) = all.iter().find(|t| t.name == name) else {
        return Err((-32602, format!("Unknown tool: {name}")));
    };
    let empty = json!({});
    let args = match params.at("arguments") {
        Value::Null => &empty,
        given => given,
    };
    let failed =
        |message: String| json!({"content": [{"type": "text", "text": message}], "isError": true});
    if !args.is_object() {
        return Ok(failed(format!(
            "Error executing tool {name}: the arguments must be an object"
        )));
    }
    if let Some(problem) = problem_with(tool, args) {
        return Ok(failed(format!("Error executing tool {name}: {problem}")));
    }
    // whatever a tool prints stays inside its answer: standard output carries protocol messages only
    let (result, _stray) = crate::out::capture(|| (tool.call)(args));
    Ok(match result {
        Ok(value) => {
            json!({"content": [{"type": "text", "text": Json::TWO.dumps(&value)}], "structuredContent": value, "isError": false})
        }
        Err(err) => failed(format!("Error executing tool {name}: {err:#}")),
    })
}

fn answer(method: &str, params: &Value) -> Result<Value, Refusal> {
    match method {
        "initialize" => {
            let asked = params.s("protocolVersion");
            let version = if VERSIONS.contains(&asked) {
                asked
            } else {
                VERSIONS[0]
            };
            Ok(json!({"protocolVersion": version,
                      "capabilities": {"tools": {"listChanged": false}, "resources": {"subscribe": false, "listChanged": false},
                                       "prompts": {"listChanged": false}},
                      "serverInfo": {"name": "council-loop", "version": crate::assets::VERSION},
                      "instructions": INSTRUCTIONS}))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tools().iter().map(describe).collect::<Vec<_>>()})),
        "tools/call" => call_tool(params),
        "resources/list" => Ok(json!({"resources": [
            {"uri": SKILL_URI, "name": "skill_md", "description": "The Council Loop skill (Agent Skills format). Read this first.", "mimeType": "text/plain"},
            {"uri": LOCK_URI, "name": "models_lock",
             "description": "Current model picks for every mode x profile (generated from Artificial Analysis data).", "mimeType": "text/plain"}]})),
        "resources/templates/list" => Ok(json!({"resourceTemplates": [
            {"uriTemplate": "council://reference/{name}", "name": "reference",
             "description": "Reference docs: protocol, context-and-memory, model-modes, research, commands.", "mimeType": "text/plain"},
            {"uriTemplate": "council://templates/{name}", "name": "template",
             "description": "Templates: brief.yaml, final-writeup.md, run.example.json, and the seat packets ideate, vote,\n\
                             redteam, elder and control (e.g. council://templates/ideate).", "mimeType": "text/plain"}]})),
        "resources/read" => {
            let uri = params.s("uri");
            match read_resource(uri) {
                Some(text) => {
                    Ok(json!({"contents": [{"uri": uri, "mimeType": "text/plain", "text": text}]}))
                }
                None => Err((-32002, format!("Unknown resource: {uri}"))),
            }
        }
        "prompts/list" => Ok(
            json!({"prompts": [{"name": "council_loop", "description": PROMPT_ABOUT, "arguments": [
            {"name": "task", "required": true}, {"name": "tier", "required": false}, {"name": "mode", "required": false},
            {"name": "profile", "required": false}]}]}),
        ),
        "prompts/get" => {
            if params.s("name") != "council_loop" {
                return Err((-32602, format!("Unknown prompt: {}", params.s("name"))));
            }
            let args = params.at("arguments");
            if text(args, "task").is_none() {
                return Err((-32602, "Missing required arguments: task".to_string()));
            }
            let (prompt, _stray) = crate::out::capture(|| prompt_text(args));
            Ok(
                json!({"description": PROMPT_ABOUT, "messages": [{"role": "user", "content": {"type": "text", "text": prompt}}]}),
            )
        }
        other => Err((-32601, format!("Method not found: {other}"))),
    }
}

/// The reply to one message, or `None` when it wants none (a notification, or a reply of the client's own).
fn handle(message: &Value) -> Option<Value> {
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        if message.has("result") || message.has("error") {
            return None; // a reply of the client's own
        }
        return Some(
            json!({"jsonrpc": "2.0", "id": message.at("id"), "error": {"code": -32600, "message": "Invalid Request: no method"}}),
        );
    };
    let id = message.get("id").filter(|id| !id.is_null())?.clone();
    Some(match answer(method, message.at("params")) {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err((code, text)) => {
            json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": text}})
        }
    })
}

/// `council mcp`: answer MCP requests on standard input until it closes.
pub fn serve() -> Result<i32> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(Value::Array(batch)) if batch.is_empty() => Some(
                json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32600, "message": "Invalid Request: an empty batch"}}),
            ),
            Ok(Value::Array(batch)) => {
                let replies: Vec<Value> = batch.iter().filter_map(handle).collect();
                (!replies.is_empty()).then_some(Value::Array(replies))
            }
            Ok(message) => handle(&message),
            Err(err) => Some(
                json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("Parse error: {err}")}}),
            ),
        };
        if let Some(reply) = reply {
            // serde_json never puts a raw line break inside a string, so one message is one line
            writeln!(stdout, "{}", serde_json::to_string(&reply)?)?;
            stdout.flush()?;
        }
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(method: &str, params: Value) -> Value {
        handle(&json!({"jsonrpc": "2.0", "id": 7, "method": method, "params": params}))
            .expect("a request gets a reply")
    }

    #[test]
    fn the_handshake_offers_a_known_protocol_revision() {
        let reply = ask(
            "initialize",
            json!({"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}}),
        );
        assert_eq!(reply["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(reply["result"]["serverInfo"]["name"], "council-loop");
        assert_eq!(
            ask("initialize", json!({"protocolVersion": "1999-01-01"}))["result"]
                ["protocolVersion"],
            VERSIONS[0]
        );
        assert_eq!(
            ask("ping", json!({})),
            json!({"jsonrpc": "2.0", "id": 7, "result": {}})
        );
    }

    #[test]
    fn notifications_and_stray_replies_get_no_answer() {
        assert!(
            handle(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).is_none()
        );
        assert!(handle(
            &json!({"jsonrpc": "2.0", "method": "tools/call", "params": {"name": "council_doctor"}})
        )
        .is_none());
        assert!(handle(&json!({"jsonrpc": "2.0", "id": 3, "result": {}})).is_none());
        assert_eq!(ask("no/such", json!({}))["error"]["code"], -32601);
        assert_eq!(
            handle(&json!({"jsonrpc": "2.0", "id": 4})).unwrap()["error"]["code"],
            -32600
        );
    }

    #[test]
    fn every_tool_is_listed_with_a_schema() {
        let listed = ask("tools/list", json!({}));
        let all = listed["result"]["tools"].as_array().unwrap();
        assert_eq!(all.len(), 23);
        let init = all.iter().find(|t| t["name"] == "council_init").unwrap();
        assert_eq!(
            init["inputSchema"]["required"],
            json!(["ask", "project_dir"])
        );
        assert_eq!(
            init["inputSchema"]["properties"]["project_dir"],
            json!({"type": "string", "title": "Project Dir"})
        );
        assert_eq!(
            init["inputSchema"]["properties"]["loops"],
            json!({"anyOf": [{"type": "integer"}, {"type": "null"}], "default": null, "title": "Loops"})
        );
        assert_eq!(
            init["inputSchema"]["properties"]["tier"],
            json!({"default": "auto", "type": "string", "title": "Tier"})
        );
        let locks = all
            .iter()
            .find(|t| t["name"] == "council_verify_locks")
            .unwrap();
        assert!(locks["inputSchema"].get("required").is_none());
    }

    #[test]
    fn a_call_with_bad_arguments_is_an_answer_not_a_crash() {
        let missing = ask(
            "tools/call",
            json!({"name": "council_next", "arguments": {}}),
        );
        assert_eq!(missing["result"]["isError"], true);
        assert!(missing["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("project_dir: this argument is required"));
        let wrong = ask(
            "tools/call",
            json!({"name": "council_blind", "arguments": {"cards": "nope"}}),
        );
        assert!(wrong["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("cards: must be a list of objects"));
        let relative = ask(
            "tools/call",
            json!({"name": "council_status", "arguments": {"project_dir": "relative/path"}}),
        );
        assert_eq!(relative["result"]["structuredContent"]["ok"], false);
        assert_eq!(
            ask("tools/call", json!({"name": "nope"}))["error"]["code"],
            -32602
        );
    }

    #[test]
    fn building_blocks_answer_with_structured_content() {
        let cards = json!([{"id": "a", "title": "One", "body": "w ".repeat(100), "eli5": "simple", "author": "L1"},
                           {"id": "b", "title": "Two", "body": "w ".repeat(100), "eli5": "plain", "author": "L2"}]);
        let blind = ask(
            "tools/call",
            json!({"name": "council_blind", "arguments": {"cards": cards, "seed": 3}}),
        );
        let result = &blind["result"];
        assert_eq!(result["isError"], false);
        assert_eq!(result["structuredContent"]["ok"], true);
        let text: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(&text, &result["structuredContent"]);
    }

    #[test]
    fn resources_and_the_prompt_are_served() {
        let listed = ask("resources/list", json!({}));
        assert_eq!(listed["result"]["resources"][0]["uri"], SKILL_URI);
        let skill = ask("resources/read", json!({"uri": SKILL_URI}));
        assert!(skill["result"]["contents"][0]["text"]
            .as_str()
            .unwrap()
            .contains("name: council-loop"));
        let packet = ask(
            "resources/read",
            json!({"uri": "council://templates/ideate"}),
        );
        assert!(packet["result"]["contents"][0]["text"]
            .as_str()
            .unwrap()
            .contains("IDEATE"));
        let missing = ask(
            "resources/read",
            json!({"uri": "council://templates/no-such-thing"}),
        );
        assert_eq!(
            missing["result"]["contents"][0]["text"],
            "(missing: no-such-thing)"
        );
        for uri in [
            "council://templates/..%2F..%2Fsecret",
            "council://reference/a/b",
            "council://reference/..",
            "file:///etc/passwd",
        ] {
            assert_eq!(
                ask("resources/read", json!({"uri": uri}))["error"]["code"],
                -32002,
                "{uri}"
            );
        }
        assert_eq!(leaf("a%20b.md").as_deref(), Some("a b.md"));
        assert_eq!(leaf("c%2Fd.md"), None);
        assert_eq!(
            ask("prompts/list", json!({}))["result"]["prompts"][0]["name"],
            "council_loop"
        );
        assert_eq!(
            ask(
                "prompts/get",
                json!({"name": "council_loop", "arguments": {}})
            )["error"]["code"],
            -32602
        );
    }
}
