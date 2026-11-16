//! `council demo`: a whole council on a made-up question, played by a script.
//!
//! ELI5: a flight simulator. The real cockpit, the real checklists, a pretend sky. Every command
//! is the real one, every rule is enforced as in a real run, and the votes are really counted.
//! Only the people at the table are a script: no model is called, nothing costs anything and
//! nothing leaves the machine. It takes a few seconds and shows what a run looks like on disk.
//!
//! The demo runs this same program once per step, in a folder of its own with settings of its own,
//! so it never touches your projects, your settings or your shared memory. The script in
//! [`script`] plays the chair (the brief, the merge, the fuse, the write-up) and every seat (it
//! reads the packet the tool wrote and answers it in the format the packet asks for). The same
//! script drives the test suite's whole-run scenarios.

use crate::core::fail;
use crate::py::{self, Json};
use crate::val::{items, Get};
use anyhow::{Context, Result};
use regex::Regex;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// The question the scripted council decides.
pub const ASK: &str = "Pick the queue technology for the billing pipeline";

/// One command that was run: its exit code and what it printed.
pub struct Played {
    /// The exit code (`-1` when the program was stopped by a signal).
    pub code: i32,
    /// Standard output.
    pub out: String,
    /// Standard error.
    pub err: String,
}

/// A project folder with settings of its own, and the program that is run in it.
pub struct Stage {
    /// The project folder.
    pub dir: PathBuf,
    /// The folder that stands in for your own settings and private folders (outside the project).
    pub own: PathBuf,
    exe: PathBuf,
    /// Environment variables set (or, with `None`, removed) for every command, after the stage's own.
    env: Vec<(String, Option<String>)>,
}

impl Stage {
    /// A stage in `dir` (created when missing) with its settings in `own`, driven by `exe` (this
    /// program, in the demo).
    pub fn new(dir: &Path, own: &Path, exe: &Path) -> Result<Stage> {
        for folder in [dir.join(".council"), own.to_path_buf()] {
            std::fs::create_dir_all(&folder)
                .with_context(|| format!("cannot create {}", folder.display()))?;
        }
        Ok(Stage {
            dir: dunce::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf()),
            own: dunce::canonicalize(own).unwrap_or_else(|_| own.to_path_buf()),
            exe: exe.to_path_buf(),
            env: Vec::new(),
        })
    }

    /// Set (or, with `None`, remove) an environment variable for every command of this stage.
    pub fn with_env(mut self, name: &str, value: Option<&str>) -> Stage {
        self.env.push((name.to_string(), value.map(str::to_string)));
        self
    }

    /// Run `council <args>` in the project, cut off from this machine's own settings and memory.
    pub fn council(&self, args: &[&str]) -> Result<Played> {
        let mut cmd = std::process::Command::new(&self.exe);
        cmd.args(args)
            .current_dir(&self.dir)
            .stdin(std::process::Stdio::null())
            .env("COUNCIL_HOME", self.own.join("home"))
            .env("COUNCIL_PRIVATE_DIR", self.own.join("private"))
            .env("COUNCIL_NO_WELCOME", "1")
            .env("COUNCIL_SEATS_VIA", "subagents")
            .env("CLAUDECODE", "1");
        for name in [
            "COUNCIL_MEMORY_DIR",
            "COUNCIL_OBSIDIAN_VAULT",
            "COUNCIL_OBSIDIAN_FOLDER",
            "COUNCIL_SKILL_DIR",
            "COUNCIL_LOG",
            "COUNCIL_LOG_FILE",
            "OPENROUTER_API_KEY",
        ] {
            cmd.env_remove(name);
        }
        for (name, value) in &self.env {
            match value {
                Some(value) => cmd.env(name, value),
                None => cmd.env_remove(name),
            };
        }
        let done = cmd
            .output()
            .with_context(|| format!("cannot run {}", self.exe.display()))?;
        Ok(Played {
            code: done.status.code().unwrap_or(-1),
            out: String::from_utf8_lossy(&done.stdout).into_owned(),
            err: String::from_utf8_lossy(&done.stderr).into_owned(),
        })
    }

    /// The folder of the current run.
    pub fn run_dir(&self) -> Result<PathBuf> {
        let runs = self.dir.join(".council").join("runs");
        let id = crate::core::read_text(&runs.join("CURRENT")).context("no run was started")?;
        Ok(runs.join(py::strip(&id)))
    }

    /// The state of the current run.
    pub fn run(&self) -> Result<Value> {
        let file = self.run_dir()?.join("run.json");
        Ok(serde_json::from_str(&crate::core::read_text(&file)?)?)
    }
}

/// What a chair and its seats write during a run, as a script would write it.
pub mod script {
    use super::*;

    /// The cards the scripted experts hold: title, body (90 to 110 words) and ELI5.
    pub const CARDS: [(&str, &str, &str); 6] = [
        ("Managed queue with idempotency keys",
         "Put every billing event on a managed queue and give each event an idempotency key made from the account id and the invoice number. The consumer writes the key and the ledger entry in one database transaction, so a retried delivery finds the key and does nothing. The queue gives retries, a dead letter queue and metrics without new servers to run. Order is kept per account by using the account id as the message group. The facts on file say duplicates fell to zero once keys were added. The cost is a monthly fee and one new table for the keys.",
         "A mail service that may deliver twice, and a notebook that stops us paying twice."),
        ("Outbox table inside the billing database",
         "Write each billing event into an outbox table in the same transaction as the order, then let one small worker read the table in order and post to the ledger. Nothing new is bought and nothing can be lost between the order and the event, because both are one commit. The worker marks a row done only after the ledger accepts it, and the ledger refuses an invoice number it has seen. Throughput is bounded by the database, which the stated peak of four thousand events per second would strain. It is the simplest thing to run and to reason about.",
         "Keep a to-do list in the same notebook as the orders, and tick items off in order."),
        ("Log stream partitioned by account",
         "Append every billing event to a log stream that is split into partitions by account id. Each partition keeps its own order, which is what billing needs, and consumers can replay from any point after a bug. The ledger consumer stores the last offset it applied together with its write, so a restart continues exactly where it stopped. This handles the stated peak with room to grow and gives an audit trail for free. The price is real: a cluster to operate, partitions to size, and a team that must learn how consumer groups rebalance before the first incident.",
         "One long diary per customer that can be read again from any page."),
        ("Keep the nightly batch, add a ledger check",
         "Change nothing in how events move. Keep the nightly batch that bills from the orders table, and add one check that compares the ledger with the orders before the batch commits. A mismatch stops the batch and pages the person on call. This costs a few days, needs no new system and removes the double charge that started this discussion, because the batch already runs once. It does not give billing within minutes, which the requester may want later, and a failed night delays every invoice. It is the option to beat: anything larger must show what it adds.",
         "Do the sums once a night and check them before sending the bills."),
        ("Two lanes: fast queue and audit log",
         "Send each billing event down two paths. A managed queue feeds the ledger within seconds, with idempotency keys so that a retry is harmless. In parallel the same event is appended to cheap object storage as an audit log that is never edited. Once a day a job compares the ledger with the audit log and reports any gap. The fast path gives speed, the slow path gives proof, and neither depends on the other being right. The cost is two things to run and a daily report that someone must read. Order per account comes from the queue's message groups.",
         "A quick courier for the bills and a slow, careful archive that checks the courier."),
        ("Use the payment provider's event feed",
         "Let the payment provider be the source of billing events. Its webhook feed already carries an event id, retries for three days and signs every message. The service stores each event id in the ledger transaction and ignores one it has seen, so retries cannot double charge. There is no queue to choose, run or pay for. The risk is dependence: a provider outage or a change to the feed stops billing, events for other payment methods need a second path, and order across events is not promised, so the ledger must accept late arrivals by invoice number.",
         "Let the bank tell us what was paid, and write down each message only once."),
    ];

    /// The one-shot answer of the control arm: what a single model would say without a council.
    pub const CONTROL: (&str, &str, &str) = (
        "Single consumer and a unique invoice index",
        "Keep the service as it is and make double charges impossible in the ledger itself. Add a unique index on the invoice number, so a second write of the same invoice fails, and treat that failure as success. Run one consumer process so that events are applied in the order they were written. This is a day of work and no new system. It leaves the delivery path fragile: a crash between reading an order and writing the ledger still loses the event until the nightly job finds it, and one consumer caps throughput well under the stated peak of four thousand events per second.",
        "Make the ledger refuse to write the same bill twice.",
    );

    /// Idea lines (25 words at most each) the scripted experts draw from.
    pub const IDEAS: [&str; 16] = [
        "Managed queue plus idempotency keys: retries become harmless, which is the top rubric item.",
        "Outbox table in the orders database: no event can be lost between the order and the bill.",
        "Log stream partitioned by account: order per account and replay after a bug.",
        "Keep the nightly batch and add a ledger check: smallest change that stops double charges.",
        "Fast queue plus an append-only audit log: speed and proof from two independent paths.",
        "Take events from the payment provider's feed: nothing new to run.",
        "Deduplicate in the ledger by invoice number only: one unique index, no new parts.",
        "Database triggers that call the ledger: simple, but ties billing to one database.",
        "A single consumer with a lease: no parallel workers, so no ordering problem at all.",
        "Contrarian: bill on read, computing charges from orders whenever an invoice is opened.",
        "Contrarian: do not queue at all, call the ledger in the request and retry with a key.",
        "Simpler: a cron job every five minutes instead of nightly, same code as today.",
        "Simpler: one table of pending events polled by the service that already runs.",
        "Shadow mode first: run the new path beside the old one and compare for two weeks.",
        "Per-account locks around billing: order is kept without any partitioning.",
        "A reconciliation report as the only safety net: detect and refund double charges fast.",
    ];

    /// The order in which the scripted judges like the cards (by title), best first.
    const LIKED: [usize; 6] = [0, 4, 1, 2, 5, 3];

    /// What the scripted red team finds in each card: severity, finding, evidence, fix.
    const FINDINGS: [(&str, &str, &str, &str); 6] = [
        (
            "S2",
            "the table of keys grows without end and slows the transaction it protects",
            "suspected: the design names no expiry",
            "expire keys after the queue's retention period",
        ),
        (
            "S1",
            "at the stated peak one worker falls behind and bills arrive hours late",
            "probe: polling single rows tops out near 1,500 per second",
            "read in batches and run one worker per shard",
        ),
        (
            "S2",
            "nobody on the team has run such a cluster: the first rebalance stops billing",
            "suspected: no runbook is named",
            "use a managed offering and rehearse a rebalance first",
        ),
        (
            "S1",
            "one failed check delays every invoice by a day, with no partial run",
            "reproduced on paper: the batch is all or nothing",
            "bill the accounts that reconcile and hold only the rest",
        ),
        (
            "S3",
            "the daily report has no owner and will go unread",
            "suspected",
            "page on a gap instead of mailing a report",
        ),
        (
            "S1",
            "bank transfers never appear in the provider's feed, so those bills are never written",
            "the feed covers card payments only",
            "add a second path for other payment methods",
        ),
    ];

    static OUT_RX: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"Write ONLY (?:this JSON|the Markdown document) to the file `(.+?)`")
            .expect("valid")
    });
    static CANDIDATE_RX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?m)^### Candidate ([A-Z]+): (.*)$").expect("valid"));
    static RUBRIC_RX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?m)^\| (R\d+) \|").expect("valid"));
    static SEAT_RX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"· seat (\w+)").expect("valid"));
    static NUMBER_RX: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"Write at least (\d+) distinct ideas|Keep your best (\d+)").expect("valid")
    });
    static SKELETON_RX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"skeleton made by a script: `(.+?)`").expect("valid"));
    static FILE_RX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?m)^### (?:\d+\. )?`([^`]+)`").expect("valid"));

    /// The file a packet asks its seat to write.
    pub fn out_path(packet: &str) -> Option<PathBuf> {
        OUT_RX.captures(packet).map(|m| PathBuf::from(&m[1]))
    }

    /// The seat a packet is addressed to.
    pub fn seat_of(packet: &str) -> String {
        SEAT_RX
            .captures(packet)
            .map(|m| m[1].to_string())
            .unwrap_or_default()
    }

    /// The candidates of a packet in the order THIS seat sees them: label and title.
    pub fn candidates(packet: &str) -> Vec<(String, String)> {
        CANDIDATE_RX
            .captures_iter(packet)
            .map(|m| (m[1].to_string(), m[2].trim().to_string()))
            .collect()
    }

    /// The rubric ids of a packet.
    pub fn rubric_ids(packet: &str) -> Vec<String> {
        let mut ids: Vec<String> = RUBRIC_RX
            .captures_iter(packet)
            .map(|m| m[1].to_string())
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }

    /// How well the script likes a card, by its title: lower is better. A card it does not know
    /// (the control arm, a fused card with a new title) comes after the ones it knows.
    fn liking(title: &str) -> usize {
        CARDS
            .iter()
            .position(|(t, _, _)| *t == title)
            .and_then(|card| LIKED.iter().position(|l| *l == card))
            .unwrap_or(LIKED.len())
    }

    /// Labels from best to worst as the script judges them. `tilt` swaps the top two for one judge
    /// in three, so the ballots are not all the same.
    fn ranked(packet: &str, tilt: bool) -> Vec<String> {
        let mut found = candidates(packet);
        found.sort_by_key(|(label, title)| (liking(title), label.clone()));
        let mut labels: Vec<String> = found.into_iter().map(|(label, _)| label).collect();
        if tilt && labels.len() > 2 {
            labels.swap(0, 1);
        }
        labels
    }

    /// The chair's intent brief for a run, as YAML.
    pub fn brief(run: &Value) -> String {
        let mut experts: Vec<&String> = items(run.at("seats"))
            .filter(|(_, v)| v.s("role") == "expert")
            .map(|(s, _)| s)
            .collect();
        experts.sort();
        let lenses = [
            (
                "least that can go wrong",
                "reliability for every customer",
                "the budget",
                "work back from the worst failure",
            ),
            (
                "least to build and run",
                "simplicity",
                "the stated peak load",
                "delete parts until it breaks",
            ),
            (
                "room to grow",
                "throughput and replay",
                "what the team can operate",
                "size it for ten times the load",
            ),
            (
                "cost over three years",
                "total cost",
                "reliability",
                "price every moving part",
            ),
            (
                "speed to ship",
                "time to the first safe release",
                "correctness",
                "find the smallest first step",
            ),
            (
                "the operator at night",
                "operability",
                "cost",
                "walk through one incident",
            ),
            (
                "the auditor",
                "proof of what was billed",
                "speed",
                "follow one invoice end to end",
            ),
        ];
        let lens_lines: Vec<String> = experts
            .iter()
            .enumerate()
            .map(|(i, seat)| {
                let (name, maximize, respect, method) = lenses[i % lenses.len()];
                format!(
                    "  - {{id: {seat}, name: \"{name}\", maximize: \"{maximize}\", respect: \"{respect}\", method: \"{method}\", evidence: \"the facts on file and the code\"}}"
                )
            })
            .collect();
        format!(
            "restated: \"Choose how billing events travel from an order to the ledger, so that no customer is charged twice.\"\n\
             eli5: \"Pick the safest way to carry bills from the shop to the ledger.\"\n\
             rubric:\n\
             \x20 - {{id: R1, name: \"no double charge, no lost event\", weight: 40, check: judgment}}\n\
             \x20 - {{id: R2, name: \"cost to run\", weight: 20, check: judgment}}\n\
             \x20 - {{id: R3, name: \"simple to operate\", weight: 20, check: judgment}}\n\
             \x20 - {{id: R4, name: \"time to ship\", weight: 10, check: judgment}}\n\
             \x20 - {{id: R5, name: \"order kept per account\", weight: 10, check: judgment}}\n\
             evidence_plan: [\"replay last month's events through the candidate\", \"a load test at the stated peak\"]\n\
             research_questions:\n\
             \x20 - \"How do billing systems stop a retried queue delivery from charging a customer twice?\"\n\
             \x20 - \"Which queue designs keep events in order for one account?\"\n\
             lenses:\n{}\n\
             facts: \"Peak is 4,000 events per second. Last month 0.4% of retried events were billed twice.\"\n\
             lessons_applied: []\n\
             approved_by: \"unattended\"\n",
            lens_lines.join("\n")
        )
    }

    fn research(packet: &str) -> Value {
        let seat = seat_of(packet);
        let beat = if seat == "RA" { "papers" } else { "field" };
        json!({
            "synthesis": format!("The {beat} sources agree that retries need idempotency keys and that order holds per partition only."),
            "research": [
                {"claim": format!("Idempotency keys stop double charges when a {beat} queue retries a delivery"),
                 "source": format!("https://example.org/{beat}/idempotency"), "quote": "duplicates fell from 0.4% to 0.0%",
                 "date": "2026-02-01", "kind": if seat == "RA" { "paper" } else { "blog" }, "confidence": "high",
                 "tags": ["queues", "billing"], "why": "the rubric weighs reliability highest"},
                {"claim": format!("Ordering is kept within one partition only, per the {beat} sources"),
                 "source": format!("https://example.org/{beat}/ordering"), "quote": "order is guaranteed per partition",
                 "date": "2025-09-12", "kind": "docs", "confidence": "medium", "tags": ["queues"],
                 "why": "billing needs order per account"},
                {"claim": format!("A managed queue caps one message group at about 300 messages per second, per the {beat} sources"),
                 "source": format!("https://example.org/{beat}/throughput"), "quote": "300 messages per second per message group",
                 "date": "2025-11-20", "kind": "docs", "confidence": "medium", "tags": ["queues", "limits"],
                 "why": "order per account bounds how fast one account can be billed"},
                {"claim": format!("An outbox table polled by one worker carried about 1,500 events per second in the {beat} tests"),
                 "source": format!("https://example.org/{beat}/outbox"), "quote": "1,500 events per second on one node",
                 "date": "2024-06-30", "kind": if seat == "RA" { "paper" } else { "blog" }, "confidence": "medium",
                 "tags": ["outbox", "limits"], "why": "the stated peak is 4,000 events per second"},
                {"claim": "At least once delivery is the default in every queue that was surveyed here",
                 "source": "https://example.org/shared/delivery", "quote": "at-least-once by default", "date": "2025-01-05",
                 "kind": "docs", "confidence": "high", "tags": ["queues"], "why": "both researchers can find the same fact"}],
            "gaps": ["no public cost figures above ten thousand events per second"],
            "conflicts": [format!("One {beat} source prices a managed queue at half of what another does: the newer one is trusted more")],
            "queries": [format!("{beat} queue idempotency keys"), "billing queue ordering"],
            "notes": [{"to": "all", "text": format!("Two {beat} sources disagree on cost: treat any price claim as unverified")}],
            "eli5": "retries are safe only with keys", "friction": ""})
    }

    /// A codebase map built from the skeleton the mapper's packet points to.
    pub fn codebase_map(packet: &str) -> Option<String> {
        let skeleton = crate::core::read_text(Path::new(&SKELETON_RX.captures(packet)?[1])).ok()?;
        let named: Vec<String> = FILE_RX
            .captures_iter(&skeleton)
            .map(|m| m[1].to_string())
            .take(6)
            .collect();
        let first = named.first()?.clone();
        let mut lines: Vec<String> = [
            "# Architecture", "",
            "> ELI5: a small service that writes each billing event down once.", "",
            "## Purpose and philosophy",
            "A small billing service. An order comes in, a charge is worked out, and the charge is written to a ledger exactly once. The code is kept small on purpose: every part does one job, and nothing reaches around the queue to write the ledger directly. That rule is what makes a double charge a bug that can be found in one place.", "",
            "## Bird's-eye view", "```mermaid", "flowchart LR", "  api --> queue --> ledger", "```",
            "Requests enter, wait, and are written down.", "",
            "## Code map",
        ]
        .iter()
        .map(|l| (*l).to_string())
        .collect();
        lines.extend(
            named
                .iter()
                .map(|n| format!("- `{n}`: one job, and it must not reach around the queue.")),
        );
        lines.extend(
            [
                "",
                "## Interfaces",
                "- The command line and the functions the skeleton lists.",
                "",
                "## Data and control flow",
                "An event arrives, is queued, and is written to the ledger once.",
                "",
                "## Invariants",
                "- An event is never written twice.",
                "",
                "## Extending it",
                "- A new consumer registers with the queue and nothing else changes.",
                "",
                "## Performance and cost",
                "- The ledger write is the slow step.",
                "",
                "## Where to change what",
                "| To do this | Start in |",
                "| --- | --- |",
            ]
            .iter()
            .map(|l| (*l).to_string()),
        );
        lines.push(format!("| add a consumer | `{first}` |"));
        lines.push(String::new());
        Some(lines.join("\n"))
    }

    fn ideate(packet: &str) -> Value {
        let seat = seat_of(packet);
        let index = seat
            .trim_start_matches(|c: char| !c.is_ascii_digit())
            .parse::<usize>()
            .unwrap_or(1)
            .saturating_sub(1);
        let (mut n, mut keep) = (5usize, 2usize);
        for m in NUMBER_RX.captures_iter(packet) {
            if let Some(found) = m.get(1) {
                n = found.as_str().parse().unwrap_or(n);
            }
            if let Some(found) = m.get(2) {
                keep = found.as_str().parse().unwrap_or(keep);
            }
        }
        let ideas: Vec<&str> = (0..n + 1)
            .map(|i| IDEAS[(index * 5 + i) % IDEAS.len()])
            .collect();
        let cards: Vec<Value> = (0..keep)
            .map(|i| {
                let (title, body, eli5) = CARDS[(index * keep + i) % CARDS.len()];
                json!({"title": title, "body": body, "wins": ["R1"], "evidence": "the facts on file; no test was run", "eli5": eli5})
            })
            .collect();
        json!({"lens": seat, "eli5": "start from what must never happen: a customer charged twice",
               "ideas": ideas, "cards": cards,
               "notes": [{"to": "research", "text": "Does any managed queue keep order across message groups, or only inside one?"}],
               "friction": ""})
    }

    fn vote(packet: &str) -> Value {
        let seat = seat_of(packet);
        // one voter in a larger field sees it a little differently: real ballots are never all alike
        let tilt = seat.ends_with('2') && candidates(packet).len() >= 5;
        let ranking = ranked(packet, tilt);
        let top = ranking.first().cloned().unwrap_or_default();
        let second = ranking.get(1).cloned().unwrap_or_default();
        let best_on: serde_json::Map<String, Value> = rubric_ids(packet)
            .into_iter()
            .map(|r| (r, json!(top)))
            .collect();
        json!({"voter": seat, "ranking": ranking,
               "reasons": {top.clone(): "R1: a retry cannot charge twice", second: "R1, R3: safe and simple to run"},
               "best_on": best_on})
    }

    fn red_team(packet: &str) -> Value {
        let mut found = candidates(packet);
        found.sort();
        let findings: Vec<Value> = found
            .iter()
            .map(|(label, title)| {
                let (severity, finding, evidence, fix) = CARDS
                    .iter()
                    .position(|(t, _, _)| t == title)
                    .map(|card| FINDINGS[card])
                    .unwrap_or(("S3", "the rollback path is not described", "suspected", "say how to turn it off"));
                json!({"candidate": label, "severity": severity, "finding": finding, "evidence": evidence, "fix": fix})
            })
            .collect();
        json!({"findings": findings, "eli5": "each plan has one way to hurt a customer: here it is"})
    }

    fn elder(packet: &str) -> Value {
        let order = ranked(packet, false);
        let ids = rubric_ids(packet);
        let scores: serde_json::Map<String, Value> = order
            .iter()
            .enumerate()
            .map(|(place, label)| {
                let score = 9usize.saturating_sub(place).max(5);
                let row: serde_json::Map<String, Value> =
                    ids.iter().map(|r| (r.clone(), json!(score))).collect();
                (label.clone(), Value::Object(row))
            })
            .collect();
        let verdict: serde_json::Map<String, Value> = order
            .iter()
            .enumerate()
            .map(|(place, label)| {
                (
                    label.clone(),
                    json!(if place == 0 { "ship" } else { "revise" }),
                )
            })
            .collect();
        let preferred = order.first().cloned().unwrap_or_default();
        json!({"scores": scores, "verdict": verdict, "preferred": preferred, "confidence": 0.8,
               "top_risks": [{"risk": "the table of idempotency keys needs an expiry rule before launch", "severity": "S2", "candidate": preferred}],
               "key_fix": "write the key and the ledger entry in one transaction, and test it with a forced retry",
               "own_idea": "replay last month's events through the new path before any customer sees it",
               "eli5": "the safest plan wins; fix the one loose end first"})
    }

    /// What a seat answers to its packet: the text of its output file. `None` for a packet the
    /// script does not know.
    pub fn answer(packet: &str) -> Option<String> {
        let head = packet.lines().next().unwrap_or("");
        let object = if head.contains("· CODEBASE MAP ·") {
            return codebase_map(packet);
        } else if head.contains("· RESEARCH ·") {
            research(packet)
        } else if head.contains("· IDEATE ·") {
            ideate(packet)
        } else if head.contains("· VOTE (") {
            vote(packet)
        } else if head.contains("· RED TEAM ·") {
            red_team(packet)
        } else if head.contains("· ELDER REVIEW ·") {
            elder(packet)
        } else if head.contains("· CONTROL ARM ·") {
            json!({"title": CONTROL.0, "body": CONTROL.1, "eli5": CONTROL.2})
        } else {
            return None;
        };
        Some(Json::ONE.dumps(&object))
    }

    /// Every seat with a packet in `folder` answers it. Returns the seats that wrote an output.
    pub fn play_seats(folder: &Path) -> Result<Vec<String>> {
        let mut packets: Vec<PathBuf> = std::fs::read_dir(folder)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.to_string_lossy().ends_with(".prompt.md"))
            .collect();
        packets.sort();
        let mut done = Vec::new();
        for file in packets {
            let packet = crate::core::read_text(&file)?;
            let (Some(out), Some(text)) = (out_path(&packet), answer(&packet)) else {
                return fail(format!("the script has no answer for {}", file.display()));
            };
            if out.exists() {
                continue;
            }
            std::fs::write(&out, text)
                .with_context(|| format!("cannot write {}", out.display()))?;
            done.push(seat_of(&packet));
        }
        Ok(done)
    }

    fn edit_json(file: &Path, change: impl FnOnce(&mut Value)) -> Result<()> {
        let mut value: Value = serde_json::from_str(&crate::core::read_text(file)?)
            .with_context(|| format!("{} is not JSON", file.display()))?;
        change(&mut value);
        std::fs::write(file, Json::ONE.dumps(&value))
            .with_context(|| format!("cannot write {}", file.display()))
    }

    /// The chair reads the merged pool and approves it.
    pub fn approve_pool(file: &Path) -> Result<()> {
        edit_json(file, |pool| pool["reviewed"] = json!(true))
    }

    /// The chair answers every red-team finding in the fused candidates.
    pub fn fill_fuse(file: &Path) -> Result<()> {
        edit_json(file, |fused| {
            if let Some(responses) = fused["responses"].as_array_mut() {
                for response in responses {
                    response["action"] = json!("fixed");
                    response["note"] = json!("the fix the red team named is now part of the card");
                }
            }
            fused["checks"] = json!({});
        })
    }

    /// The chair fills in a document the tool drafted. Every `TODO(chair)` (with the hint after
    /// it, up to the end of its table cell or line) becomes the text whose marker its line holds.
    /// A text may hold one answer per cell, separated by ` || `: a line with fewer cells to fill
    /// takes the last ones.
    pub fn fill_todos(file: &Path, lines: &[(&str, &str)], extra: &str) -> Result<()> {
        static TODO_RX: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"TODO\(chair\)[^|\n]*").expect("valid"));
        let text = crate::core::read_text(file)?;
        let filled: Vec<String> = text
            .lines()
            .map(|line| {
                let with = lines
                    .iter()
                    .find(|(marker, _)| line.contains(marker))
                    .map_or("see the ledger of this run", |(_, with)| with);
                let answers: Vec<&str> = with.split(" || ").collect();
                let cells = TODO_RX.find_iter(line).count();
                let mut next = answers.len().saturating_sub(cells);
                TODO_RX
                    .replace_all(line, |found: &regex::Captures| {
                        let answer = answers[next.min(answers.len() - 1)];
                        next += 1;
                        // inside a table the cell keeps its closing bar apart
                        let cell = line[found.get(0).map_or(0, |m| m.end())..].starts_with('|');
                        format!("{answer}{}", if cell { " " } else { "" })
                    })
                    .into_owned()
            })
            .collect();
        std::fs::write(file, format!("{}\n{extra}", filled.join("\n")))
            .with_context(|| format!("cannot write {}", file.display()))
    }
}

/// The files of the made-up project the demo council decides about.
const PROJECT: [(&str, &str); 6] = [
    ("README.md", "# Shop\n\nA toy billing service: the made-up project `council demo` holds its council about.\n"),
    ("docs/decisions.md", "# Decisions\n\nNone yet: the council's first one is about how billing events travel.\n"),
    ("shop/__init__.py", "\"\"\"The shop package.\"\"\"\n"),
    ("shop/core.py", "\"\"\"Billing: charge an order.\"\"\"\nimport os\n\nfrom . import db\nfrom .queue import BillingQueue\n\n\ndef bill(order, retries: int = 2):\n    \"\"\"Charge it.\"\"\"\n    queue = BillingQueue(os.environ.get(\"QUEUE_URL\"))\n    queue.post_event(order)\n    return db.save(order)\n"),
    ("shop/db.py", "\"\"\"Storage.\"\"\"\n\n\ndef save(order):\n    \"\"\"Write one order.\"\"\"\n    return order\n\n\nclass Ledger:\n    \"\"\"Every charge, once.\"\"\"\n\n    def post_entry(self, entry):\n        return entry\n"),
    ("shop/queue.py", "\"\"\"The queue of billing events.\"\"\"\n\n\nclass BillingQueue:\n    def __init__(self, url):\n        self.url = url\n\n    def post_event(self, event):\n        return event\n"),
];

/// Write the made-up project into `dir`.
pub fn write_project(dir: &Path) -> Result<()> {
    for (name, text) in PROJECT {
        let file = dir.join(name);
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&file, text).with_context(|| format!("cannot write {}", file.display()))?;
    }
    Ok(())
}

/// One step of the play: the command (when there is one), what the script did, what came back.
pub struct Step {
    /// The command line, as a person would type it (empty for a step the script takes alone).
    pub command: String,
    /// What happened, in one line.
    pub note: String,
    /// The command's output.
    pub played: Option<Played>,
}

/// Play one whole run on `stage`, from `init` to `done`. `init` holds the flags after the ask.
/// `each` is told every step. Stops with an error when a command fails or the run stops moving.
///
/// The loop is the one a chair follows: `ingest` checks what was written and prepares the next
/// step, so `next` is needed only where nothing has been prepared yet (and after a restart).
pub fn play(stage: &Stage, init: &[&str], each: impl FnMut(&Step)) -> Result<()> {
    play_with(stage, init, false, each)
}

/// [`play`], with a choice of who answers the packets: the script itself (as subagents would, by
/// writing output files), or, with `dispatch`, the command-line tools on the PATH through
/// `council dispatch` (in the test suite those tools are stand-ins that answer with this script).
pub fn play_with(
    stage: &Stage,
    init: &[&str],
    dispatch: bool,
    mut each: impl FnMut(&Step),
) -> Result<()> {
    let command = |args: &[&str], note: &str, each: &mut dyn FnMut(&Step)| -> Result<()> {
        let played = stage.council(args)?;
        let shown: Vec<String> = args
            .iter()
            .map(|a| {
                if a.contains(' ') {
                    format!("\"{a}\"")
                } else {
                    (*a).to_string()
                }
            })
            .collect();
        let line = format!("council {}", shown.join(" "));
        let failed = (played.code != 0).then(|| {
            format!(
                "`{line}` ended with code {}:\n{}{}",
                played.code, played.out, played.err
            )
        });
        each(&Step {
            command: line,
            note: note.to_string(),
            played: Some(played),
        });
        match failed {
            Some(why) => fail(why),
            None => Ok(()),
        }
    };
    let said = |note: String, each: &mut dyn FnMut(&Step)| {
        each(&Step {
            command: String::new(),
            note,
            played: None,
        });
    };
    let mut first = vec!["init", ASK];
    first.extend_from_slice(init);
    command(
        &first,
        "the gate sizes the work and seats the council",
        &mut each,
    )?;
    for _ in 0..120 {
        let run = stage.run()?;
        let d = stage.run_dir()?;
        let (phase, at) = (run.s("phase").to_string(), run.show("loop"));
        let loop_dir = d.join(format!("loop-{at}"));
        // what the step needs on disk before anyone can act; `next` writes it when it is not there
        let prepared = match phase.as_str() {
            "brief" => d.join("brief.yaml"),
            "merge" => loop_dir.join("pool.json"),
            "fuse" => loop_dir.join("candidates.json"),
            "final" => d.join("final.md"),
            "retro" => d.join("retro.md"),
            "research" | "ideate" | "vote" | "rank" | "redteam" | "elders" => loop_dir.join(&phase),
            _ => d.join("run.json"),
        };
        if !prepared.exists() {
            command(
                &["next"],
                "the tool prepares the step and says what to do",
                &mut each,
            )?;
        }
        match phase.as_str() {
            "done" => return Ok(()),
            "direct" => return fail("the script plays a council; the gate chose the direct tier"),
            "brief" => {
                std::fs::write(d.join("brief.yaml"), script::brief(&run))?;
                said("the chair fills in the brief: the question restated, a rubric written before anyone has ideas, one lens per expert".into(), &mut each);
                command(
                    &["ingest"],
                    "the brief is checked and frozen; the first packets are written",
                    &mut each,
                )?;
            }
            "research" | "ideate" | "vote" | "rank" | "redteam" | "elders" => {
                if dispatch {
                    command(
                        &["dispatch"],
                        "the seats behind command-line tools are sent their packets",
                        &mut each,
                    )?;
                    command(
                        &["ingest"],
                        "every answer is checked against its packet's rules",
                        &mut each,
                    )?;
                    continue;
                }
                let mut seats = script::play_seats(&loop_dir.join(&phase))?;
                if phase == "ideate" && d.join("control").join("CTRL.prompt.md").exists() {
                    seats.extend(script::play_seats(&d.join("control"))?);
                }
                let who = match phase.as_str() {
                    "research" => "the researchers bring sourced facts; the mapper writes the code map",
                    "ideate" => "every expert works alone on the same ask through its own lens; the control arm gives a one-shot answer",
                    "vote" | "rank" => "every voter ranks the blinded cards, each in a different order",
                    "redteam" => "the red team attacks the blinded shortlist",
                    _ => "the elders review the blinded finalists",
                };
                said(format!("{who} ({})", seats.join(", ")), &mut each);
                command(
                    &["ingest"],
                    "every answer is checked against its packet's rules",
                    &mut each,
                )?;
            }
            "merge" => {
                script::approve_pool(&loop_dir.join("pool.json"))?;
                said("the chair reviews the drafted pool (same ideas merged, authors out of sight) and approves it".into(), &mut each);
                command(&["ingest"], "the pool is accepted", &mut each)?;
            }
            "fuse" => {
                script::fill_fuse(&loop_dir.join("candidates.json"))?;
                said("the chair answers every finding of the red team in writing: fixed, accepted or refuted".into(), &mut each);
                command(&["ingest"], "no finding may be left unanswered", &mut each)?;
            }
            "cut" => {
                command(
                    &["cut", "--eli5", "one plan won on reliability; its loose end is fixed", "--changed", "the winner gained an expiry rule for its keys"],
                    "the votes are counted, the elders' verdicts weighed, the survivors recorded in the ledger",
                    &mut each,
                )?;
            }
            "final" => {
                let winner = winner_title(&stage.run()?);
                let known = script::CARDS.iter().find(|(title, _, _)| *title == winner);
                let answer = match known {
                    Some((_, body, _)) => {
                        py::head(body, body.find(". The ").map_or(240, |at| at + 1)).to_string()
                    }
                    None => {
                        "It ranked first in the blind vote and the elders said ship.".to_string()
                    }
                };
                let eli5 = known.map_or(
                    "The plan most judges ranked first, after its weak point was fixed.",
                    |(_, _, eli5)| eli5,
                );
                script::fill_todos(
                    &d.join("final.md"),
                    &[
                        ("# TODO", &format!("Go with: {winner}")),
                        ("**Answer:**", &answer),
                        ("**ELI5:**", eli5),
                        ("one diagram", "order -> queue -> consumer (key and ledger entry in one transaction) -> ledger"),
                        ("evidence with its source", "duplicates fell from 0.4% to 0.0% once keys were added (https://example.org/papers/idempotency: a made-up source of this demo)"),
                        ("Success metric", "no invoice billed twice in 30 days; 90 days without a manual correction"),
                        ("Kill criterion", "one double charge after launch: stop and fall back to the nightly batch"),
                        ("- [ ]", "replay last month's events through the new path before any customer sees it"),
                        ("cut in loop", "it ranked below the winner in the blind vote"),
                        ("| S", "keys older than 90 days are removed by a nightly job, tested before launch || the team that ships it"),
                    ],
                    "\nDecided unattended where noted. Stand-in seats are labelled. The control baseline was read.\n",
                )?;
                said("the chair writes the decision for people: the answer, the evidence, what was dropped and why".into(), &mut each);
                command(
                    &["ingest"],
                    "the write-up is checked against the record",
                    &mut each,
                )?;
            }
            "retro" => {
                let stamp = crate::clock::today().format("%Y%m%d").to_string();
                script::fill_todos(
                    &d.join("retro.md"),
                    &[
                        ("ELI5:", "the council agreed quickly because the rubric was written first"),
                        ("What worked", "blind votes: nobody knew whose card was whose"),
                        ("What did not", "two researchers found the same fact twice"),
                        ("helpful or harmful", "helpful: it kept the brief short"),
                    ],
                    &format!("- [L-{stamp}-1] (helpful: 0 | harmful: 0) Write the rubric before the ideas. Evidence: this run\n"),
                )?;
                said("the chair writes what was learned (a lesson reaches docs/lessons.md only through a pull request)".into(), &mut each);
                command(&["ingest"], "the run is closed and unblinded", &mut each)?;
            }
            other => return fail(format!("the script does not know the step `{other}`")),
        }
    }
    fail("the run did not finish in 120 steps")
}

/// The title of the candidate a finished (or cut) run chose.
pub fn winner_title(run: &Value) -> String {
    let last = run.list("loops").last().cloned().unwrap_or(Value::Null);
    let id = run
        .at("winner")
        .as_str()
        .map(str::to_string)
        .or_else(|| last.list("survivors").first().map(py::to_str))
        .unwrap_or_default();
    last.list("candidates")
        .iter()
        .find(|c| c.show("id") == id)
        .map(|c| c.show("title"))
        .unwrap_or(id)
}

/// The lines of a command's output that say what it did: for `init` the gate's lines, otherwise
/// what stands above the run's header line (the results of the step that just ended).
fn headline(step: &Step, played: &Played) -> Vec<String> {
    let lines: Vec<&str> = played
        .out
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    let header = lines
        .iter()
        .position(|l| l.starts_with('[') && l.contains("] tier="));
    let picked: Vec<&str> = if step.command.starts_with("council init") {
        lines.iter().take(2).copied().collect()
    } else {
        match header {
            Some(0) => lines.iter().skip(1).take(1).copied().collect(),
            Some(at) => lines[..at].iter().rev().take(3).rev().copied().collect(),
            None => lines.iter().take(2).copied().collect(),
        }
    };
    picked
        .iter()
        .map(|l| py::head(l.trim(), 170).to_string())
        .collect()
}

/// `council demo`: play a whole council in a folder of its own and show what it left behind.
pub fn cmd_demo(dir: Option<&str>, keep: bool, verbose: bool, full: bool) -> Result<i32> {
    use crate::say;
    use std::hash::{BuildHasher as _, Hasher as _};
    let exe = std::env::current_exe().context("cannot find this program's own file")?;
    let tag = std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish() as u32;
    let (folder, made_here) = match dir {
        Some(dir) => {
            let folder = py::path(dir);
            if folder.exists() && std::fs::read_dir(&folder).is_ok_and(|mut d| d.next().is_some()) {
                return fail(format!(
                    "{} is not empty: name a new folder for the demo",
                    folder.display()
                ));
            }
            (folder, false)
        }
        None => (
            std::env::temp_dir().join(format!("council-demo-{tag:08x}")),
            true,
        ),
    };
    // the demo's own "home": settings and the private folder, never yours
    let own = std::env::temp_dir().join(format!("council-demo-{tag:08x}-own"));
    let stage = Stage::new(&folder, &own, &exe)?;
    write_project(&stage.dir)?;
    let tier = if full { "full" } else { "lite" };
    say!(
        "council demo · a whole {} council on a made-up question, played by a script.",
        if full { "Full" } else { "Lite" }
    );
    say!("No model is called, nothing is spent and nothing leaves this machine. Every command is the real one.");
    say!("project: {} (a toy billing service)", stage.dir.display());
    say!();
    let started = std::time::Instant::now();
    let mut commands = 0usize;
    let result = play(
        &stage,
        &[
            "--tier",
            tier,
            "--mode",
            "fanout:claude",
            "--profile",
            "efficient",
        ],
        |step| match &step.played {
            Some(played) => {
                commands += 1;
                say!("{commands:>2}  $ {}", step.command);
                if verbose {
                    for line in played.out.lines() {
                        say!("      > {line}");
                    }
                } else {
                    for line in headline(step, played) {
                        say!("      > {line}");
                    }
                }
            }
            None => say!("      (script) {}", step.note),
        },
    );
    let _ = std::fs::remove_dir_all(&stage.own);
    if let Err(err) = result {
        say!();
        say!("the demo stopped: {err:#}");
        say!("the folder was kept for a look: {}", stage.dir.display());
        return Ok(1);
    }
    let run = stage.run()?;
    let d = stage.run_dir()?;
    let audit = stage.council(&["audit"])?;
    let _ = std::fs::remove_dir_all(&stage.own);
    let mut path: Vec<String> = Vec::new();
    for event in crate::log::events(&d) {
        if let Some(phase) = event.list("phase").get(1).and_then(Value::as_str) {
            if path.last().map(String::as_str) != Some(phase) {
                path.push(phase.to_string());
            }
        }
    }
    say!();
    say!(
        "done: {commands} commands in {:.1} s. The run went {}.",
        started.elapsed().as_secs_f64(),
        path.join(" -> ")
    );
    say!("the decision:  {}", winner_title(&run));
    say!(
        "the audit:     {}",
        audit.out.lines().last().unwrap_or("").trim()
    );
    say!();
    let rel = |p: &Path| py::posix(p.strip_prefix(&stage.dir).unwrap_or(p));
    say!("what a run leaves behind (in {}/):", rel(&d));
    for (name, what) in [
        (
            "final.md",
            "the decision, the evidence, what was dropped and why",
        ),
        (
            &*format!("loop-{}/ledger.md", run.show("loop")),
            "the loop's record: votes, findings, verdicts, the cut",
        ),
        (
            "run.json",
            "the state: any step can be picked up again from here",
        ),
        (crate::log::EVENTS, "every step with its time (council log)"),
    ] {
        if d.join(name).exists() {
            say!("  {name:<22} {what}");
        }
    }
    for (file, what) in [
        (
            stage.dir.join("docs").join("ARCHITECTURE.md"),
            "the code map the mapper wrote",
        ),
        (
            stage.dir.join(".council").join("memory").join("index.md"),
            "the facts the researchers filed, each with its source",
        ),
    ] {
        if file.exists() {
            say!("  {:<22} {what}", rel(&file));
        }
    }
    say!();
    if keep || !made_here {
        say!(
            "the folder is kept. Look around:  cd {} && council status && council log",
            stage.dir.display()
        );
    } else {
        let _ = std::fs::remove_dir_all(&stage.dir);
        say!("the folder was removed. Keep one to look around:  council demo --keep");
    }
    say!("a real council is the same commands with real seats:  council init \"<the ask, in the requester's own words>\"");
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::script::*;

    #[test]
    fn the_scripted_cards_keep_the_rules_a_real_card_must_keep() {
        for (title, body, eli5) in CARDS.iter().chain([&CONTROL]) {
            let words = body.split_whitespace().count();
            assert!((90..=110).contains(&words), "{title}: {words} words");
            assert!(title.split_whitespace().count() <= 8, "{title}");
            assert!(eli5.split_whitespace().count() <= 20, "{eli5}");
            assert_eq!(
                crate::core::leak_in(&format!("{title} {body} {eli5}")),
                None,
                "{title}"
            );
        }
        for idea in IDEAS {
            assert!(idea.split_whitespace().count() <= 25, "{idea}");
        }
    }

    #[test]
    fn packets_are_read_the_way_a_seat_reads_them() {
        let packet = "# Council packet · run r · loop 1 · VOTE (shortlist) · seat L2\n\n| R1 | a | 40 | judgment |\n| R2 | b | 60 | judgment |\n\n\
                      ### Candidate B: Outbox table inside the billing database\nbody\nELI5: x\n\n### Candidate A: Managed queue with idempotency keys\nbody\nELI5: y\n\n\
                      ### Candidate C: Something new\nbody\nELI5: z\n\nWrite ONLY this JSON to the file `/tmp/x/L2.out.json` (create it)";
        assert_eq!(seat_of(packet), "L2");
        assert_eq!(rubric_ids(packet), ["R1", "R2"]);
        assert_eq!(
            out_path(packet).unwrap().to_string_lossy(),
            "/tmp/x/L2.out.json"
        );
        let ballot: serde_json::Value = serde_json::from_str(&answer(packet).unwrap()).unwrap();
        // by what the script likes; a card it does not know comes last
        assert_eq!(ballot["ranking"], serde_json::json!(["A", "B", "C"]));
        assert_eq!(ballot["best_on"]["R2"], "A");
        assert!(answer("# something else").is_none());
    }
}
