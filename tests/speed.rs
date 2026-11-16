//! How long things take, with limits.
//!
//! The limits are generous on purpose (ten times and more what a laptop needs): this test is not
//! here to notice a five per cent slowdown on a busy build machine. It is here to fail when
//! something becomes slow in kind: a hook that reads every locked file on every tool call, a gate
//! that compiles its patterns again for every request, a search that is quadratic.
//!
//! See the numbers: `cargo test --release --test speed -- --nocapture`. Limits are enforced on
//! optimised builds only; a debug build prints the numbers and passes.

mod common;

use common::Sandbox;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// One measurement and its limit.
struct Line {
    what: &'static str,
    took: Duration,
    limit: Duration,
}

fn ms(d: Duration) -> String {
    let ms = d.as_secs_f64() * 1000.0;
    if ms < 10.0 {
        format!("{ms:.1} ms")
    } else {
        format!("{ms:.0} ms")
    }
}

/// The median of `n` timings of `work`.
fn median(n: usize, mut work: impl FnMut()) -> Duration {
    let mut all: Vec<Duration> = (0..n)
        .map(|_| {
            let started = Instant::now();
            work();
            started.elapsed()
        })
        .collect();
    all.sort();
    all[all.len() / 2]
}

/// A project of `files` source files in three languages that import each other.
fn code(s: &Sandbox, files: usize) {
    for i in 0..files {
        let other = (i * 7 + 3) % files;
        match i % 3 {
            0 => {
                let mut text = format!(
                    "import os\nfrom pkg{}.mod{other} import helper{other}\n\n",
                    other % 20
                );
                for f in 0..12 {
                    text.push_str(&format!(
                        "def handler{i}_{f}(request, queue):\n    \"\"\"Handle step {f} of flow {i}.\"\"\"\n    total = 0\n    for item in queue:\n        total += helper{other}(item)\n    return total\n\n"
                    ));
                }
                text.push_str(&format!("class Service{i}:\n    def run(self):\n        return handler{i}_0(None, [])\n\ndef helper{i}(x):\n    return x + {i}\n"));
                s.write(&format!("pkg{}/mod{i}.py", i % 20), &text);
            }
            1 => {
                let mut text = format!(
                    "import {{ util{other} }} from '../lib{}/file{other}';\n\n",
                    other % 20
                );
                for f in 0..10 {
                    text.push_str(&format!(
                        "export function step{i}_{f}(input: number[]): number {{\n  let sum = 0;\n  for (const x of input) {{ sum += util{other}(x); }}\n  return sum;\n}}\n\n"
                    ));
                }
                text.push_str(&format!("export const util{i} = (x: number) => x * {i};\nexport class Widget{i} {{ render() {{ return step{i}_0([]); }} }}\n"));
                s.write(&format!("lib{}/file{i}.ts", i % 20), &text);
            }
            _ => {
                let mut text = String::from("use std::collections::HashMap;\n\n");
                for f in 0..10 {
                    text.push_str(&format!(
                        "pub fn compute{i}_{f}(items: &[u64]) -> u64 {{\n    let mut map: HashMap<u64, u64> = HashMap::new();\n    for item in items {{ *map.entry(*item).or_default() += 1; }}\n    map.values().sum()\n}}\n\n"
                    ));
                }
                text.push_str(&format!("pub struct Engine{i};\nimpl Engine{i} {{ pub fn go(&self) -> u64 {{ compute{i}_0(&[]) }} }}\n"));
                s.write(&format!("crates/c{}/src/part{i}.rs", i % 20), &text);
            }
        }
    }
}

#[test]
fn nothing_is_slow_in_kind() {
    let mut lines: Vec<Line> = Vec::new();
    let mut note = |what: &'static str, took: Duration, limit_ms: u64| {
        lines.push(Line {
            what,
            took,
            limit: Duration::from_millis(limit_ms),
        });
    };

    // ---- starting up: what every command, every hook call and every seat pays
    let s = Sandbox::new("speed");
    note(
        "start (council --version)",
        median(15, || assert_eq!(s.run(&["--version"]).code, 0)),
        250,
    );
    note(
        "status with no run",
        median(9, || assert_eq!(s.run(&["status"]).code, 2)),
        250,
    );

    // ---- the gate and the map on a project of 600 files (about 45,000 lines)
    code(&s, 600);
    let plan = s.run(&["plan", "Pick the queue technology for the billing pipeline"]);
    assert_eq!(plan.code, 0, "{}", plan.all());
    note(
        "plan, 600 files",
        median(3, || {
            assert_eq!(
                s.run(&["plan", "Pick the queue technology for the billing pipeline"])
                    .code,
                0
            )
        }),
        6_000,
    );
    note(
        "map skeleton, 600 files",
        median(3, || assert_eq!(s.run(&["map"]).code, 0)),
        6_000,
    );
    note(
        "map --check, 600 files",
        median(3, || assert!(s.run(&["map", "--check"]).code <= 1)),
        6_000,
    );
    let started = Instant::now();
    let init = s.run(&[
        "init",
        "Pick the queue technology for the billing pipeline",
        "--tier",
        "lite",
        "--mode",
        "fanout:claude",
        "--no-research",
    ]);
    assert_eq!(init.code, 0, "{}", init.all());
    note("init, 600 files", started.elapsed(), 8_000);
    note(
        "status of a run",
        median(9, || assert_eq!(s.run(&["status"]).code, 0)),
        300,
    );
    note(
        "next (a step already prepared)",
        median(5, || assert!(s.run(&["next"]).code <= 2)),
        500,
    );

    // ---- locked tests: locking 1,000 files, and the hook on every tool call after that
    // (the files are given an hour-old time stamp: a file written this very second is hashed
    // again by the hook, as it must be, and that is not the everyday case)
    let an_hour_ago = std::time::SystemTime::now() - Duration::from_secs(3600);
    for i in 0..1000 {
        let file = s.write(
            &format!("tests/unit/test_{i}.py"),
            &format!("def test_{i}():\n    assert {i} == {i}\n"),
        );
        let handle = std::fs::File::options()
            .write(true)
            .open(&file)
            .expect("the file was just written");
        handle
            .set_modified(an_hour_ago)
            .expect("time stamps can be set");
    }
    let started = Instant::now();
    let locked = s.run(&["lock-tests", "tests"]);
    assert_eq!(locked.code, 0, "{}", locked.all());
    note("lock-tests, 1,000 files", started.elapsed(), 3_000);
    note(
        "verify-locks, 1,000 files",
        median(3, || assert_eq!(s.run(&["verify-locks"]).code, 0)),
        3_000,
    );
    let cwd = s.proj.display().to_string();
    let edit = json!({"hook_event_name": "PreToolUse", "tool_name": "Edit", "cwd": cwd, "tool_input": {"file_path": "src/app.py"}}).to_string();
    let shell = json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "cwd": cwd,
                       "tool_input": {"command": "cd src && grep -rn TODO . | sort > /tmp/todo.txt; cargo test 2>&1 | tail -5"}})
    .to_string();
    let after = json!({"hook_event_name": "PostToolUse", "tool_name": "Bash", "cwd": cwd, "tool_input": {"command": "make test"}, "tool_response": {}}).to_string();
    let hook = |event: &str| assert_eq!(s.run_with(&["hook"], &[], Some(event)).code, 0);
    note(
        "hook before an edit, 1,000 locks",
        median(15, || hook(&edit)),
        300,
    );
    note(
        "hook before a shell line, 1,000 locks",
        median(15, || hook(&shell)),
        300,
    );
    hook(&after); // the first look reads every locked file once
    note(
        "hook after a shell line, 1,000 locks",
        median(15, || hook(&after)),
        400,
    );

    // ---- the shared memory: 150 facts, then a search
    for i in 0..150 {
        let claim = format!(
            "Queue {i} keeps order within partition {} under load level {}",
            i % 7,
            i % 11
        );
        let added = s.run(&[
            "memory",
            "add",
            "--claim",
            &claim,
            "--source",
            &format!("https://example.org/queues/{i}"),
            "--tags",
            "queue,billing",
        ]);
        assert_eq!(added.code, 0, "{}", added.all());
    }
    note(
        "memory search, 150 facts",
        median(7, || {
            assert_eq!(s.run(&["memory", "search", "order partition load"]).code, 0)
        }),
        1_000,
    );
    note(
        "memory add (one fact, 150 held)",
        median(5, || {
            let n = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let claim = format!("Broker {n} acknowledges a write after two replicas hold it");
            assert_eq!(
                s.run(&[
                    "memory",
                    "add",
                    "--claim",
                    &claim,
                    "--source",
                    "https://example.org/brokers"
                ])
                .code,
                0
            );
        }),
        1_000,
    );

    // ---- a whole scripted council, and the server answering many calls
    let demo = Sandbox::new("speed-demo");
    let started = Instant::now();
    let played = demo.run(&["demo"]);
    assert_eq!(played.code, 0, "{}", played.all());
    note(
        "demo: a whole Lite council, 12 commands",
        started.elapsed(),
        10_000,
    );
    let started = Instant::now();
    let played = demo.run(&["demo", "--full"]);
    assert_eq!(played.code, 0, "{}", played.all());
    note(
        "demo --full: three loops, 31 commands",
        started.elapsed(),
        20_000,
    );

    // one server, many calls: nothing is worked out again that was worked out before
    let serve = |calls: Vec<Value>| {
        let input: String = calls.iter().map(|c| format!("{c}\n")).collect();
        let started = Instant::now();
        let served = s.run_with(&["mcp", "--project", &cwd], &[], Some(&input));
        let took = started.elapsed();
        assert_eq!(served.code, 0, "{}", served.err);
        let replies: Vec<Value> = served
            .out
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        assert_eq!(replies.len(), calls.len());
        assert!(
            replies.iter().all(|r| r.get("result").is_some()),
            "a call was refused"
        );
        took
    };
    let light: Vec<Value> = (0..300)
        .map(|id| match id % 3 {
            0 => json!({"jsonrpc": "2.0", "id": id, "method": "tools/list"}),
            1 => json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": "council_status", "arguments": {}}}),
            _ => json!({"jsonrpc": "2.0", "id": id, "method": "resources/read", "params": {"uri": "council://run/current"}}),
        })
        .collect();
    note(
        "MCP server: 300 calls (tool list, status, a resource)",
        serve(light),
        5_000,
    );
    let plans: Vec<Value> = (0..10)
        .map(|id| {
            json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
                   "params": {"name": "council_plan", "arguments": {"task": "Rotate the API keys of the billing service"}}})
        })
        .collect();
    note("MCP server: 10 plans, 600 files", serve(plans), 30_000);

    // ---- the report
    let wide = lines.iter().map(|l| l.what.len()).max().unwrap_or(0);
    println!();
    for line in &lines {
        println!(
            "{:wide$}  {:>9}   (limit {})",
            line.what,
            ms(line.took),
            ms(line.limit)
        );
    }
    println!();
    if cfg!(debug_assertions) {
        println!("this is a debug build: the limits are not enforced (run with --release)");
        return;
    }
    let slow: Vec<String> = lines
        .iter()
        .filter(|l| l.took > l.limit)
        .map(|l| format!("{}: {} (limit {})", l.what, ms(l.took), ms(l.limit)))
        .collect();
    assert!(
        slow.is_empty(),
        "slower than the limit:\n{}",
        slow.join("\n")
    );
}
