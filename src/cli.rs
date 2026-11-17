//! The command line: one verb per step of the loop, plus the building blocks on their own.

use crate::core::{self, MemberFlags, ResearchFlag, UserError};
use crate::py::Json;
use crate::val::Get;
use crate::{complain, say};
use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use serde_json::Value;
use std::path::Path;

const ABOUT: &str = "council - the one enforcement path for the Council Loop

ELI5: the AI does the thinking; this program does the bookkeeping that should never be left to an
AI's good intentions: remembering where the run is, hiding who wrote what, shuffling, counting
votes, cutting 7 -> 5 -> 3 -> 1, checking the rules were followed, and locking test files.

Every surface (Claude Code skill, plugin hook, MCP server, AGENTS.md tools, CI) calls these same
functions, so the rules are enforced the same way everywhere.

Start here
  demo          a whole council on a made-up question, played by a script: no model, no cost
  doctor        what can be called from here (CLIs, keys, stale data) and which mode fits

Run loop (resumable; state lives in .council/runs/<id>/run.json)
  init          start a run: gate, seats, files. Prints the GATE line
  next          prepare the current step (writes prompt packets) and say exactly what to do
  dispatch      run the seats that live behind a CLI or API (codex, gemini, openrouter, claude -p)
  ingest        validate what came back; advance when it is all valid (--drop names a seat to go on without)
  cut           tally + elders + wildcard -> survivors; writes the loop ledger
  status        one-screen digest of the run and what is still missing (use it after a context reset)
  deviation     record that the run left the protocol, and why
  audit         re-count every vote from the ballots on disk and compare with the record
  log           the run's diary: every step taken and every seat called, with times

Building blocks (also used on their own)
  plan          dry-run the gate (no tokens spent)
  models        model picks for a mode/profile from models/lock.yaml
  blind         anonymise + shuffle candidate cards into a judge packet (seeded), enforce caps
  tally         count blind ballots as head-to-heads weighted by stake; checks beat votes; agreement; cut
  check         validate a run ledger against the protocol
  lock-tests    record sha256 of files agents may not edit (tests, migrations, ...)
  verify-locks  fail if a locked file changed (use --against <git ref> in CI)
  ask / elders  one prompt to one provider / one blinded packet to the elder council
  refresh       rebuild models/lock.yaml from Artificial Analysis data
  api           the same operations as JSON in, JSON out (council api --list)
  mcp           serve the API to any MCP client over stdio
  hook          the Claude Code hook that guards locked files and keeps council seats in their lane
Run `council <command> -h` for options.";

#[derive(Parser)]
#[command(name = "council", version, about = "The Council Loop: gate, seats, blind votes, cuts, locks", long_about = ABOUT)]
#[command(infer_long_args = true, disable_help_subcommand = true)]
struct Cli {
    /// Say more about what is being done, on standard error: -v decisions, -vv every read, write and call.
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count, global = true)]
    verbose: u8,
    /// How much to say: off, error, warn (default), info, debug, trace. Also the COUNCIL_LOG variable.
    #[arg(long, global = true, value_name = "LEVEL", value_parser = ["off", "error", "warn", "info", "debug", "trace"])]
    log_level: Option<String>,
    #[command(subcommand)]
    cmd: Cmd,
}

/// The gate's inputs, shared by `plan` and `init`.
#[derive(Args, Clone, Debug)]
pub struct GateOpts {
    /// The size of the work; `auto` lets the gate choose.
    #[arg(long, default_value = "auto", value_parser = ["auto", "direct", "lite", "full"])]
    pub tier: String,
    /// solo | crosslab | fanout:<claude|gemini|openai|openrouter|open|mix>  (default: the cheapest
    /// that can be reached from here, as `doctor` shows; or `defaults.mode` in your settings)
    #[arg(long)]
    pub mode: Option<String>,
    /// How much to spend on models (default: efficient, the cheapest; or `defaults.profile` in your settings).
    #[arg(long, value_parser = ["max", "balanced", "efficient"])]
    pub profile: Option<String>,
    /// Loop cap (default 3 for full, 1 for lite). Only the requester raises it.
    #[arg(long, allow_negative_numbers = true)]
    pub loops: Option<i64>,
    /// The chair's judgment, recorded.
    #[arg(long, value_parser = ["low", "medium", "high"])]
    pub stakes: Option<String>,
    /// The chair's judgment, recorded.
    #[arg(long, value_parser = ["yes", "no"])]
    pub reversible: Option<String>,
    /// The requester's own words when they override the gate, e.g. "lite is fine".
    #[arg(long = "override", allow_hyphen_values = true)]
    pub override_words: Option<String>,
    /// Files the task will touch (path tripwires).
    #[arg(long, num_args = 0..)]
    pub paths: Vec<String>,
    /// How many researchers, 0 to 4 (default 2, or what the project's config says).
    #[arg(long, value_name = "N", allow_negative_numbers = true)]
    pub research: Option<i64>,
    /// Same as --research 0.
    #[arg(long)]
    pub no_research: bool,
    /// Searches or page reads per researcher (default 10 lite, 15 full).
    #[arg(long, value_name = "N", allow_negative_numbers = true)]
    pub research_budget: Option<i64>,
    /// Research again before later loops: when a member asks (default), always, or never.
    #[arg(long, value_parser = ["on_request", "always", "never"])]
    pub research_later: Option<String>,
    /// When the codebase mapper runs: only when the map is missing or behind the code (default), always, never.
    #[arg(long = "map", value_parser = ["stale", "always", "never"])]
    pub mapper: Option<String>,
    /// Same as --map never.
    #[arg(long)]
    pub no_map: bool,
    /// Size of the ranked list of code that packets carry when the ask names code (default 700; 0 = none).
    #[arg(long, value_name = "N", allow_negative_numbers = true)]
    pub focus_tokens: Option<i64>,
}

impl GateOpts {
    /// The settings these flags ask for the support members.
    pub fn member_flags(&self) -> MemberFlags {
        MemberFlags {
            research: if self.no_research {
                Some(ResearchFlag::Seats(0))
            } else {
                self.research.map(ResearchFlag::Seats)
            },
            mapper: if self.no_map {
                Some("never".to_string())
            } else {
                self.mapper.clone()
            },
            research_budget: self.research_budget,
            research_later: self.research_later.clone(),
            focus_tokens: self.focus_tokens,
        }
    }

    /// The gate's inputs without the ask.
    pub fn gate(&self) -> crate::gate::GateArgs {
        crate::gate::GateArgs {
            tier: self.tier.clone(),
            mode: self.mode.clone(),
            profile: self.profile.clone(),
            loops: self.loops,
            paths: self.paths.clone(),
            stakes: self.stakes.clone(),
            reversible: self.reversible.clone(),
            override_words: self.override_words.clone(),
            flags: self.member_flags(),
        }
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// What can be called from here; recommended mode.
    Doctor {
        /// Send one tiny test prompt to each installed CLI.
        #[arg(long)]
        probe: bool,
    },
    /// Dry-run the gate (no tokens spent).
    Plan {
        /// The ask, in the requester's own words.
        task: String,
        #[command(flatten)]
        gate: GateOpts,
    },
    /// Start a run (prints the GATE line).
    Init {
        /// The ask, verbatim, in the requester's own words.
        ask: String,
        #[command(flatten)]
        gate: GateOpts,
        /// Keep this run's shared memory in your Obsidian vault (set it once: memory setup --obsidian).
        #[arg(long)]
        obsidian: bool,
    },
    /// Prepare the current step and say what to do.
    Next {
        /// Run id (default: the current run).
        #[arg(long)]
        run: Option<String>,
        /// The sealed control arm is gone for good: go on without it (logged).
        #[arg(long, value_name = "WHAT_HAPPENED", allow_hyphen_values = true)]
        without_control: Option<String>,
    },
    /// One-screen digest of the run.
    Status {
        /// Run id (default: the current run).
        #[arg(long)]
        run: Option<String>,
    },
    /// Run the CLI/API seats of the current step.
    Dispatch {
        /// Run id (default: the current run).
        #[arg(long)]
        run: Option<String>,
        /// Only this seat.
        #[arg(long)]
        seat: Option<String>,
        /// Print the commands that would run, and run nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Validate outputs of the current step; advance when valid.
    Ingest {
        /// Run id (default: the current run).
        #[arg(long)]
        run: Option<String>,
        /// Go on without these seats (comma-separated), e.g. L3 or EB,EC.
        #[arg(long, value_name = "SEATS")]
        drop: Option<String>,
        /// Why they are left out (logged as a deviation).
        #[arg(long, allow_hyphen_values = true)]
        reason: Option<String>,
    },
    /// Re-count every vote from the ballots on disk; exit 1 on a mismatch.
    Audit {
        /// Run id (default: the current run).
        #[arg(long)]
        run: Option<String>,
    },
    /// The run's diary: every step taken and every seat called, with times.
    Log {
        /// Run id (default: the current run).
        #[arg(long)]
        run: Option<String>,
        /// Only the last N events.
        #[arg(long, short = 'n', value_name = "N")]
        tail: Option<usize>,
        /// One JSON object per line, as the file holds them.
        #[arg(long)]
        json: bool,
    },
    /// Record that the run left the protocol, and why.
    Deviation {
        /// One line: what was done differently, and why.
        text: String,
        /// Run id (default: the current run).
        #[arg(long)]
        run: Option<String>,
    },
    /// Tally + elders + wildcard -> survivors; writes the loop ledger.
    Cut {
        /// Run id (default: the current run).
        #[arg(long)]
        run: Option<String>,
        /// One line: what happened in this loop.
        #[arg(long, allow_hyphen_values = true)]
        eli5: String,
        /// One line: what changed since the last loop.
        #[arg(long, default_value = "", allow_hyphen_values = true)]
        changed: String,
        /// Stop early: converged or stalled (keeps 1).
        #[arg(long, value_name = "REASON", allow_hyphen_values = true)]
        stop: Option<String>,
        /// The one question put to the requester and the answer: "Q -> A".
        #[arg(long, value_name = "Q_AND_A", allow_hyphen_values = true)]
        asked: Option<String>,
        /// Elders split or an S1 is open and the requester cannot be asked: the safer option you are taking.
        #[arg(long, value_name = "SAFER_OPTION", allow_hyphen_values = true)]
        unattended: Option<String>,
    },
    /// Show every setting for the researchers, the mapper and the shared memory, and who set it.
    Config {
        /// Print a commented example config file instead.
        #[arg(long)]
        example: bool,
    },
    /// Post to the run's commons: a thought for the members or a question for the researchers.
    Note {
        /// The note: one thought, or one question.
        text: String,
        /// Who it is for: every member, the chair only, or a question for the researchers.
        #[arg(long, default_value = "all", value_parser = ["all", "chair", "research"])]
        to: String,
        /// Run id (default: the current run).
        #[arg(long)]
        run: Option<String>,
    },
    /// The shared memory: research notes (facts with sources) and the commons.
    Memory {
        #[command(subcommand)]
        cmd: MemoryCmd,
    },
    /// The codebase map: write the skeleton, or check / validate / accept the map.
    Map {
        /// Exit 1 when the map is missing or behind the code (for CI).
        #[arg(long)]
        check: bool,
        /// Check the written map: sections, and every path it names.
        #[arg(long)]
        validate: bool,
        /// Record that the map describes the code as it is now.
        #[arg(long)]
        accept: bool,
        /// Write the skeleton here (default .council/map/skeleton.md).
        #[arg(long)]
        out: Option<String>,
        /// Skeleton length in lines.
        #[arg(long, default_value_t = 420)]
        budget: i64,
        /// Write the mapper's packet, to refresh the map without a council run.
        #[arg(long)]
        packet: bool,
        /// Validate a written map and put it in place (records what it was checked against).
        #[arg(long, value_name = "FILE")]
        install: Option<String>,
        /// Print the code that matters most for a task described in TEXT (ranked, about 700 tokens).
        #[arg(long, value_name = "TEXT")]
        focus: Option<String>,
        /// Say which code readers this build has.
        #[arg(long)]
        parsers: bool,
    },
    /// Show model picks.
    Models {
        /// solo, crosslab or fanout:<claude|gemini|openai|openrouter|open|mix>.
        #[arg(long, default_value = "crosslab")]
        mode: String,
        /// How much to spend on models.
        #[arg(long, default_value = crate::gate::DEFAULT_PROFILE, value_parser = ["max", "balanced", "efficient"])]
        profile: String,
    },
    /// Anonymise + shuffle cards.
    Blind {
        /// JSON file or string: list of cards.
        cards: String,
        /// The shuffle's seed: the same seed gives the same order.
        #[arg(long, default_value_t = 0)]
        seed: u64,
        /// The longest a card's body may be, in words.
        #[arg(long, default_value_t = core::caps::CARD)]
        max_words: usize,
        /// Write packet markdown here; mapping goes to <out>.mapping.json.
        #[arg(long)]
        out: Option<String>,
    },
    /// Count blind ballots (head-to-heads weighted by stake) + cut.
    Tally {
        /// JSON file or string: the ballots, each {voter, ranking, best_on}.
        votes: String,
        /// JSON file or string: the private mapping that `blind` wrote (label -> card and authors).
        mapping: String,
        /// JSON file or string: label -> {criterion: score}, from machine checks or judges.
        #[arg(long)]
        scores: Option<String>,
        /// JSON file or string: label -> "pass" or "fail". A failed check ranks last.
        #[arg(long)]
        checks: Option<String>,
        /// How many to keep (default: rank only).
        #[arg(long)]
        keep: Option<usize>,
    },
    /// Validate a run ledger JSON.
    Check {
        /// Path to run.json (default: the current run).
        run: Option<String>,
    },
    /// Lock files/dirs agents may not edit.
    LockTests {
        /// Files or folders to lock (a folder locks every file in it).
        #[arg(required = true)]
        paths: Vec<String>,
    },
    /// Exit 1 if a locked file changed.
    VerifyLocks {
        /// Read the manifest from this ref (use in CI).
        #[arg(long, value_name = "GIT_REF")]
        against: Option<String>,
    },
    /// One prompt to one provider.
    Ask {
        /// Whose model to ask.
        #[arg(long, value_parser = ["claude", "openai", "gemini", "openrouter"])]
        provider: String,
        /// The model's id as that provider names it (default: the provider's own default).
        #[arg(long)]
        model: Option<String>,
        /// Reasoning effort, where the provider has such a setting.
        #[arg(long)]
        effort: Option<String>,
        /// The file that holds the prompt.
        #[arg(long)]
        prompt_file: String,
        /// Print the command that would run, and run nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Blinded packet -> elder council (stand-alone use).
    Elders {
        /// The file that holds the blinded packet (Markdown).
        #[arg(long)]
        packet: String,
        /// How much to spend on models.
        #[arg(long, default_value = crate::gate::DEFAULT_PROFILE, value_parser = ["max", "balanced", "efficient"])]
        profile: String,
        /// Which providers to ask, comma-separated.
        #[arg(long, default_value = "claude,openai,gemini")]
        providers: String,
        /// The folder for each provider's review.
        #[arg(long, default_value = ".council/elders")]
        out: String,
        /// Print the commands that would run, and run nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Rebuild models/lock.yaml from Artificial Analysis.
    Refresh {
        /// Snapshot JSON instead of the live API.
        #[arg(long)]
        offline: Option<String>,
        /// Exit 1 when the model data is stale.
        #[arg(long)]
        check: bool,
        /// Print the table of picks for a profile.
        #[arg(long, value_parser = ["max", "balanced", "efficient"])]
        table: Option<String>,
        /// Write the lock file here instead of the skill's models folder.
        #[arg(long)]
        out: Option<String>,
    },
    /// Play a whole council on a made-up question with a script for every seat: no model, no cost.
    Demo {
        /// Play it in this (new or empty) folder and keep it.
        #[arg(long, value_name = "FOLDER")]
        dir: Option<String>,
        /// Keep the folder the demo made, to look around.
        #[arg(long)]
        keep: bool,
        /// Play a Full council (three loops, seven experts) instead of a Lite one.
        #[arg(long)]
        full: bool,
        /// Print every line each command printed.
        #[arg(long)]
        all: bool,
    },
    /// Serve the API to any MCP client over stdio (tools, resources, prompts).
    Mcp {
        /// Serve this one project: project_dir may be left out, other projects are refused.
        #[arg(long, value_name = "DIR")]
        project: Option<String>,
        /// Offer only the operations that change nothing.
        #[arg(long)]
        read_only: bool,
        /// Never offer an operation that sends prompts to model vendors.
        #[arg(long)]
        no_dispatch: bool,
        /// Groups of tools to offer: run, memory, map, blocks or all (default run,memory,map).
        #[arg(long, value_name = "SETS")]
        toolsets: Option<String>,
        /// Offer exactly these operations (comma-separated; `council api --list` names them).
        #[arg(long, value_name = "NAMES")]
        tools: Option<String>,
        /// Never offer these operations.
        #[arg(long, value_name = "NAMES")]
        deny_tools: Option<String>,
        /// List each tool's output schema too (a larger tool list for the client to carry).
        #[arg(long)]
        output_schemas: bool,
        /// Trim tool and parameter prose descriptions to shrink tools/list wire size while keeping all tools and schemas.
        #[arg(long)]
        compact_schemas: bool,
    },
    /// The API surface from a shell: list the operations, describe them, or call one with JSON.
    Api {
        /// The operation to call or describe (see --list).
        operation: Option<String>,
        /// The arguments, as one JSON object.
        #[arg(long, value_name = "JSON", allow_hyphen_values = true)]
        args: Option<String>,
        /// Read the arguments from this file (- for standard input).
        #[arg(long, value_name = "FILE", allow_hyphen_values = true)]
        args_file: Option<String>,
        /// List the operations.
        #[arg(long)]
        list: bool,
        /// Print the description (input and output schemas) of one operation, or of all.
        #[arg(long)]
        describe: bool,
    },
    /// The Claude Code hook: reads the event on stdin and denies edits to locked files.
    Hook,
}

#[derive(Subcommand)]
enum MemoryCmd {
    /// Where the memory lives and what it holds.
    Status,
    /// Choose where the memory lives (saved in YOUR settings unless --project).
    Setup {
        /// Keep it in an Obsidian vault.
        #[arg(long)]
        obsidian: bool,
        /// Path of the vault folder.
        #[arg(long)]
        vault: Option<String>,
        /// Folder inside the vault (default "Council Loop/<project>").
        #[arg(long)]
        folder: Option<String>,
        /// Plain files (default: .council/memory in the project).
        #[arg(long)]
        files: bool,
        /// Folder for --files.
        #[arg(long = "dir")]
        directory: Option<String>,
        /// Save in the project's .council/config.yaml instead.
        #[arg(long)]
        project: bool,
    },
    /// Add one fact by hand.
    Add {
        /// One fact, 45 words at most.
        #[arg(long, allow_hyphen_values = true)]
        claim: String,
        /// A web address, a file in the project, or 'test: <command>'.
        #[arg(long, allow_hyphen_values = true)]
        source: String,
        /// The exact words or number from the source.
        #[arg(long, default_value = "", allow_hyphen_values = true)]
        quote: String,
        /// paper, docs, blog, repo, code, data, talk, test or other.
        #[arg(long, default_value = "other")]
        kind: String,
        /// How far the source can be trusted.
        #[arg(long, default_value = "medium", value_parser = ["high", "medium", "low"])]
        confidence: String,
        /// Comma-separated.
        #[arg(long, default_value = "")]
        tags: String,
        /// When the source was published.
        #[arg(long, default_value = "")]
        date: String,
        /// Why it matters here.
        #[arg(long, default_value = "", allow_hyphen_values = true)]
        why: String,
        /// Who adds it (a name or a seat).
        #[arg(long, default_value = "chair")]
        by: String,
    },
    /// Find facts by keyword.
    Search {
        /// Keywords.
        query: String,
        /// Only facts with this tag.
        #[arg(long)]
        tag: Option<String>,
        /// The most facts to show.
        #[arg(long, default_value_t = 10, allow_negative_numbers = true)]
        limit: i64,
        /// Also show facts that were retired.
        #[arg(long)]
        retired: bool,
    },
    /// Mark a fact as no longer held (it is kept, with the reason, not deleted).
    Retire {
        /// The note's file name, without .md.
        note: String,
        /// Why it no longer holds (at least three words).
        #[arg(long, allow_hyphen_values = true)]
        because: String,
        /// The note that replaces it.
        #[arg(long)]
        by: Option<String>,
    },
    /// Bring research notes from another memory folder into this one.
    Import {
        /// The folder to import from.
        #[arg(long = "from")]
        src: String,
    },
}

fn dump(value: &Value) -> String {
    Json::ONE.dumps(value)
}

fn run_cmd(cmd: Cmd) -> Result<i32> {
    match cmd {
        Cmd::Doctor { probe } => {
            let out = crate::gate::doctor(None, probe);
            say!(
                "recommended mode: {} · fresh contexts via: {}",
                out.s("recommended_mode"),
                out.s("fresh_contexts")
            );
            for n in out.list("notes") {
                say!("note: {}", crate::py::to_str(n));
            }
            say!("{}", dump(&out));
            Ok(0)
        }
        Cmd::Plan { task, gate } => {
            let mut out = crate::gate::plan(&task, &gate.gate());
            say!("{}", out.s("gate"));
            say!(
                "{}",
                crate::gate::members_line(out.at("support"), out.s("tier"), None)
            );
            if let Some(map) = out.as_object_mut() {
                map.shift_remove("gate");
            }
            say!("{}", dump(&out));
            Ok(0)
        }
        Cmd::Config { example } => {
            if example {
                let text = crate::assets::read("templates/config.example.yaml").unwrap_or_default();
                say!("{}", text.trim_end_matches(crate::py::is_space));
                return Ok(0);
            }
            let root = core::project_root();
            let settings = core::members(&root, &MemberFlags::default());
            let mem = crate::memory::settings(Some(&root));
            say!(
                "{}",
                core::members_report(&settings, &mem, &root).join("\n")
            );
            Ok(0)
        }
        Cmd::Map {
            check,
            validate,
            accept,
            out,
            budget,
            packet,
            install,
            focus,
            parsers,
        } => {
            if packet {
                return crate::run::cmd_map_packet();
            }
            crate::codemap::cmd_map(&crate::codemap::MapArgs {
                check,
                accept,
                validate,
                out,
                budget,
                install,
                parsers,
                focus,
            })
        }
        Cmd::Memory { cmd } => match cmd {
            MemoryCmd::Status => crate::memory::cmd_status(),
            MemoryCmd::Setup {
                obsidian,
                vault,
                folder,
                files,
                directory,
                project,
            } => crate::memory::cmd_setup(
                obsidian,
                vault.as_deref(),
                folder.as_deref(),
                files,
                directory.as_deref(),
                project,
            ),
            MemoryCmd::Add {
                claim,
                source,
                quote,
                kind,
                confidence,
                tags,
                date,
                why,
                by,
            } => crate::memory::cmd_add(
                &claim,
                &source,
                &quote,
                &kind,
                &confidence,
                &tags,
                &date,
                &why,
                &by,
            ),
            MemoryCmd::Search {
                query,
                tag,
                limit,
                retired,
            } => crate::memory::cmd_search(&query, tag.as_deref(), limit, retired),
            MemoryCmd::Retire { note, because, by } => {
                crate::memory::cmd_retire(&note, &because, by.as_deref())
            }
            MemoryCmd::Import { src } => crate::memory::cmd_import(&src),
        },
        Cmd::Init {
            ask,
            gate,
            obsidian,
        } => crate::run::cmd_init(&ask, &gate.gate(), obsidian),
        Cmd::Note { text, to, run } => crate::run::cmd_note(run.as_deref(), &text, &to),
        Cmd::Next {
            run,
            without_control,
        } => crate::run::cmd_next(run.as_deref(), without_control.as_deref()),
        Cmd::Audit { run } => crate::run::cmd_audit(run.as_deref()),
        Cmd::Log { run, tail, json } => crate::run::cmd_log(run.as_deref(), tail, json),
        Cmd::Status { run } => crate::run::cmd_status(run.as_deref()),
        Cmd::Dispatch { run, seat, dry_run } => {
            crate::run::cmd_dispatch(run.as_deref(), seat.as_deref(), dry_run)
        }
        Cmd::Ingest { run, drop, reason } => {
            crate::run::cmd_ingest(run.as_deref(), drop.as_deref(), reason.as_deref())
        }
        Cmd::Deviation { text, run } => crate::run::cmd_deviation(run.as_deref(), &text),
        Cmd::Cut {
            run,
            eli5,
            changed,
            stop,
            asked,
            unattended,
        } => crate::run::cmd_cut(
            run.as_deref(),
            &eli5,
            &changed,
            stop.as_deref(),
            asked.as_deref(),
            unattended.as_deref(),
        ),
        Cmd::Models { mode, profile } => {
            say!("{}", dump(&core::models_for(&mode, &profile)));
            Ok(0)
        }
        Cmd::Blind {
            cards,
            seed,
            max_words,
            out,
        } => {
            let cards = core::read_json_arg(&cards)?;
            let result = core::blind(
                cards.as_array().map(Vec::as_slice).unwrap_or(&[]),
                seed,
                max_words,
                &[],
                1.5,
            );
            if let Some(path) = out {
                // paths the person named are theirs to choose
                crate::safe::write_named(Path::new(&path), result.s("packet"))?;
                crate::safe::write_named(
                    Path::new(&format!("{path}.mapping.json")),
                    &dump(result.at("mapping")),
                )?;
                say!("packet -> {path}; mapping (keep away from judges) -> {path}.mapping.json");
            } else {
                say!("{}", dump(&result));
            }
            for e in result.list("errors") {
                complain!("error: {}", crate::py::to_str(e));
            }
            Ok(if result.truthy("ok") { 0 } else { 2 })
        }
        Cmd::Tally {
            votes,
            mapping,
            scores,
            checks,
            keep,
        } => {
            let votes = core::read_json_arg(&votes)?;
            let mapping = core::read_json_arg(&mapping)?;
            let scores = scores.as_deref().map(core::read_json_arg).transpose()?;
            let checks = checks.as_deref().map(core::read_json_arg).transpose()?;
            let opt = core::TallyOptions {
                scores: scores.as_ref(),
                checks: checks.as_ref(),
                keep,
                ..Default::default()
            };
            say!(
                "{}",
                dump(&core::tally(
                    votes.as_array().map(Vec::as_slice).unwrap_or(&[]),
                    &mapping,
                    &opt
                ))
            );
            Ok(0)
        }
        Cmd::Check { run } => {
            let data = match run {
                Some(arg) => core::read_json_arg(&arg)?,
                None => crate::run::load(None)?.0,
            };
            let out = core::check_run(&data);
            say!("{}", dump(&out));
            Ok(if out.truthy("ok") { 0 } else { 1 })
        }
        Cmd::LockTests { paths } => {
            say!(
                "{}",
                Json::LINE.dumps(&core::lock_files(&paths, &core::project_root())?)
            );
            Ok(0)
        }
        Cmd::VerifyLocks { against } => {
            let out = core::verify_locks(&core::project_root(), against.as_deref())?;
            say!("{}", Json::LINE.dumps(&out));
            Ok(if out.truthy("ok") { 0 } else { 1 })
        }
        Cmd::Ask {
            provider,
            model,
            effort,
            prompt_file,
            dry_run,
        } => {
            let prompt = core::read_named(Path::new(&prompt_file))?;
            let out = crate::providers::ask(
                &provider,
                &prompt,
                model.as_deref(),
                effort.as_deref(),
                900,
                dry_run,
                "none",
            );
            say!("{}", dump(&out));
            Ok(if out.truthy("ok") { 0 } else { 1 })
        }
        Cmd::Elders {
            packet,
            profile,
            providers,
            out,
            dry_run,
        } => {
            let text = core::read_named(Path::new(&packet))?;
            let names: Vec<String> = providers
                .split(',')
                .map(crate::py::strip)
                .filter(|p| !p.is_empty())
                .map(str::to_string)
                .collect();
            let result = crate::providers::elder_review(
                &text,
                &profile,
                &names,
                Some(Path::new(&out)),
                dry_run,
                true,
            );
            say!("{}", dump(&result));
            Ok(if result.truthy("ok") { 0 } else { 1 })
        }
        Cmd::Refresh {
            offline,
            check,
            table,
            out,
        } => crate::refresh::run(offline.as_deref(), check, table.as_deref(), out.as_deref()),
        Cmd::Demo {
            dir,
            keep,
            full,
            all,
        } => crate::demo::cmd_demo(dir.as_deref(), keep, all, full),
        Cmd::Mcp {
            project,
            read_only,
            no_dispatch,
            toolsets,
            tools,
            deny_tools,
            output_schemas,
            compact_schemas,
        } => {
            crate::mcp::set_compact_schemas(compact_schemas);
            crate::mcp::serve(crate::mcp::Options::resolve(
                project.as_deref(),
                read_only,
                no_dispatch,
                toolsets.as_deref(),
                tools.as_deref(),
                deny_tools.as_deref(),
                output_schemas,
            )?)
        }
        Cmd::Api {
            operation,
            args,
            args_file,
            list,
            describe,
        } => crate::api::cmd_api(
            operation.as_deref(),
            args.as_deref(),
            args_file.as_deref(),
            list,
            describe,
        ),
        Cmd::Hook => Ok(crate::hook::run()),
    }
}

/// Run the command line. Returns the process exit code: 0 done, 1 a check failed or something broke,
/// 2 the user must fix something (the message says what).
pub fn main() -> i32 {
    use clap::{CommandFactory as _, FromArgMatches as _};
    if std::env::args_os().len() <= 1 {
        // no command: say what there is, and succeed. (A hook runner that drops the `hook` argument
        // must not have every tool call blocked by a usage error.)
        say!("{ABOUT}");
        return 0;
    }
    let parsed = Cli::command().try_get_matches().and_then(|matches| {
        let verb = matches.subcommand_name().unwrap_or("").to_string();
        Cli::from_arg_matches(&matches).map(|cli| (cli, verb))
    });
    let (cli, verb) = match parsed {
        Ok(found) => found,
        Err(err) => {
            let _ = err.print();
            return if err.use_stderr() { 2 } else { 0 };
        }
    };
    match (
        cli.log_level.as_deref().and_then(crate::log::Level::parse),
        cli.verbose,
    ) {
        (Some(level), _) => crate::log::set_level(level),
        (None, 1) => crate::log::set_level(crate::log::Level::Info),
        (None, 2) => crate::log::set_level(crate::log::Level::Debug),
        (None, n) if n > 2 => crate::log::set_level(crate::log::Level::Trace),
        _ => {}
    }
    crate::welcome::first_run(&verb);
    match run_cmd(cli.cmd) {
        Ok(code) => code,
        Err(err) => {
            let user = err.downcast_ref::<UserError>().is_some();
            complain!("error: {err:#}");
            if user {
                2
            } else {
                1
            }
        }
    }
}
