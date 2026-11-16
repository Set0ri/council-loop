//! Broken input must be an answer, never a crash and never a hang.
//!
//! Every reader of something a project, a seat or a client can hand this tool is fed thousands of
//! damaged versions of valid input: bytes flipped, pieces cut, repeated or swapped, values replaced
//! by the wrong kind, nesting made deep, text made long, control characters put in. The damage
//! comes from a fixed seed, so a failure can be replayed. What is checked is small and absolute:
//! no reader panics, and none takes long.

mod common;

use common::Sandbox;
use serde_json::{json, Value};
use std::cell::Cell;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Once;
use std::time::{Duration, Instant};

/// Readers called inside this process take their settings from a throwaway home, never from the
/// home of whoever runs the tests. The folder goes when the last test that uses it is over.
struct Home;

static LIVE: AtomicUsize = AtomicUsize::new(0);

fn home_dir() -> PathBuf {
    std::env::temp_dir().join(format!("council-it-{}-fuzz-home", std::process::id()))
}

impl Home {
    fn enter() -> Home {
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            std::env::set_var("COUNCIL_HOME", home_dir().join(".council"));
            std::env::set_var("COUNCIL_PRIVATE_DIR", home_dir().join(".council-private"));
        });
        LIVE.fetch_add(1, Ordering::SeqCst);
        Home
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        if LIVE.fetch_sub(1, Ordering::SeqCst) == 1 {
            let _ = std::fs::remove_dir_all(home_dir());
        }
    }
}

thread_local! {
    /// Is this thread inside [`hammer`]? A panic there is caught and reported once, with the round.
    static HAMMERING: Cell<bool> = const { Cell::new(false) };
}

/// Panics inside [`hammer`] are not printed as they happen (thousands of rounds would drown the
/// report); panics anywhere else print as usual.
fn quiet_inside_hammer() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let usual = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !HAMMERING.with(Cell::get) {
                usual(info);
            }
        }));
    });
}

/// A small random generator with a fixed seed (xorshift): the same damage on every run.
struct Dice(u64);

impl Dice {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

const POISON: [&str; 22] = [
    "",
    "\0",
    "\u{1b}[2J",
    "\u{202e}",
    "\r",
    "\n\n\n",
    "{",
    "}",
    "[",
    "]",
    "\"",
    "\\",
    "null",
    "-1",
    "1e999",
    "99999999999999999999999",
    "../../..",
    "/dev/zero",
    "\u{e0041}",
    "é",
    "&a *a",
    "---",
];

/// Damage a text: a handful of cuts, repeats, swaps and insertions at places the dice pick.
fn damaged(text: &str, dice: &mut Dice) -> String {
    let mut chars: Vec<char> = text.chars().collect();
    for _ in 0..1 + dice.below(4) {
        let at = dice.below(chars.len() + 1);
        match dice.below(7) {
            0 => chars.truncate(at),
            1 if !chars.is_empty() => {
                let end = (at + dice.below(40)).min(chars.len());
                chars.drain(at.min(end)..end);
            }
            2 => {
                let end = (at + dice.below(60)).min(chars.len());
                let piece: Vec<char> = chars[at.min(end)..end].to_vec();
                for _ in 0..1 + dice.below(50) {
                    chars.splice(at..at, piece.iter().copied());
                }
            }
            3 => {
                let poison: Vec<char> = POISON[dice.below(POISON.len())].chars().collect();
                chars.splice(at..at, poison);
            }
            4 if chars.len() > 1 => {
                let other = dice.below(chars.len());
                let at = at.min(chars.len() - 1);
                chars.swap(at, other);
            }
            5 => {
                let open = ['[', '{', '('][dice.below(3)];
                chars.splice(at..at, std::iter::repeat_n(open, 200 + dice.below(3000)));
            }
            _ => {
                let filler = ['a', ' ', '\n', 'x'][dice.below(4)];
                chars.splice(
                    at..at,
                    std::iter::repeat_n(filler, 1000 + dice.below(20_000)),
                );
            }
        }
    }
    chars.into_iter().collect()
}

/// Damage a JSON value in its structure: a value somewhere becomes one of another kind.
fn bent(value: &Value, dice: &mut Dice) -> Value {
    let wrong = || -> Vec<Value> {
        vec![
            Value::Null,
            json!(true),
            json!(-1),
            json!(1e300),
            json!(""),
            json!("\u{1b}[31m"),
            json!([]),
            json!({}),
            json!([[[[[[[[[[1]]]]]]]]]]),
            json!({"a": {"a": {"a": {"a": {}}}}}),
            json!("x".repeat(5000)),
            json!(["L1", "L1", 7, null]),
        ]
    };
    match value {
        Value::Object(map) if !map.is_empty() && dice.below(4) != 0 => {
            let key = map
                .keys()
                .nth(dice.below(map.len()))
                .cloned()
                .unwrap_or_default();
            let mut out = map.clone();
            if dice.below(8) == 0 {
                out.shift_remove(&key);
            } else {
                let inner = bent(&map[&key], dice);
                out.insert(key, inner);
            }
            Value::Object(out)
        }
        Value::Array(list) if !list.is_empty() && dice.below(4) != 0 => {
            let at = dice.below(list.len());
            let mut out = list.clone();
            out[at] = bent(&list[at], dice);
            Value::Array(out)
        }
        _ => {
            let pool = wrong();
            pool[dice.below(pool.len())].clone()
        }
    }
}

/// Run `reader` on `rounds` damaged inputs. Fails on a panic, and when the whole takes too long.
fn hammer(
    name: &str,
    seed: u64,
    rounds: usize,
    limit: Duration,
    mut reader: impl FnMut(&mut Dice),
) {
    quiet_inside_hammer();
    let mut dice = Dice(seed);
    let started = Instant::now();
    let mut broke = None;
    HAMMERING.with(|h| h.set(true));
    for round in 0..rounds {
        let before = dice.0;
        if let Err(payload) = catch_unwind(AssertUnwindSafe(|| reader(&mut dice))) {
            let said = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            broke = Some((round, before, said));
            break;
        }
    }
    HAMMERING.with(|h| h.set(false));
    if let Some((round, state, said)) = broke {
        panic!("{name}: a reader panicked in round {round} (dice state {state:#x}): {said}");
    }
    let took = started.elapsed();
    assert!(
        took < limit,
        "{name}: {rounds} rounds took {took:?} (limit {limit:?})"
    );
    println!("{name}: {rounds} damaged inputs in {took:.1?}");
}

fn seat_outputs() -> Vec<Value> {
    use council_loop::demo::script;
    let packet = |kind: &str, seat: &str| {
        format!(
            "# Council packet · run r · loop 1 · {kind} · seat {seat}\n\n| R1 | a | 60 | judgment |\n| R2 | b | 40 | judgment |\n\n\
             Write at least 5 distinct ideas. Keep your best 2.\n\n### Candidate A: {}\nbody\nELI5: x\n\n### Candidate B: {}\nbody\nELI5: y\n",
            script::CARDS[0].0, script::CARDS[1].0
        )
    };
    [
        ("RESEARCH", "RA"),
        ("IDEATE", "L1"),
        ("VOTE (rank)", "L2"),
        ("RED TEAM", "RT"),
        ("ELDER REVIEW", "EA"),
        ("CONTROL ARM", "CTRL"),
    ]
    .iter()
    .filter_map(|(kind, seat)| serde_json::from_str(&script::answer(&packet(kind, seat))?).ok())
    .collect()
}

#[test]
fn readers_of_text_survive_damaged_text() {
    let _home = Home::enter();
    let yaml = "members:\n  research:\n    seats: 3\n    budget: {lite: 7, full: 12}\n    beats:\n      - {name: Law, text: \"What the law says\"}\n\
                tripwires:\n  billing: \"\\\\bbilling\\\\b\"\ntripwire_paths: {billing: [\"shop/*\"]}\nmemory: {backend: files, dir: notes}\nbase: &b {x: 1}\nuse: {<<: *b}\n";
    hammer("yaml", 0x5eed_0001, 4000, Duration::from_secs(40), |dice| {
        let _ = council_loop::yaml::load(&damaged(yaml, dice));
    });
    let reply = "Here is my answer:\n```json\n{\"voter\": \"L1\", \"ranking\": [\"A\", \"B\"], \"reasons\": {\"A\": \"R1: \\\"best\\\" } so far\"}}\n```\nDone {x}";
    hammer(
        "parse_json",
        0x5eed_0002,
        4000,
        Duration::from_secs(40),
        |dice| {
            let text = damaged(reply, dice);
            let _ = council_loop::providers::parse_json(&text);
            let _ = council_loop::safe::unfence(&text);
            let _ = council_loop::safe::shown(&text);
            let _ = council_loop::log::redact(&text);
        },
    );
    hammer(
        "fnmatch",
        0x5eed_0003,
        6000,
        Duration::from_secs(40),
        |dice| {
            let name = damaged("src/app/billing/queue_worker.py", dice);
            let pattern = damaged("*/app/[a-c]*/queue_?orker.*", dice);
            let _ = council_loop::py::fnmatch(&name, &pattern);
        },
    );
    let map = council_loop::demo::script::CARDS
        .iter()
        .map(|c| format!("## {}\n- `src/{}.py`: {}\n", c.0, c.0.len(), c.1))
        .collect::<String>();
    let s = Sandbox::new("fuzz-map");
    s.write("src/app.py", "def main():\n    return 1\n");
    let root = s.proj.clone();
    hammer(
        "validate_map",
        0x5eed_0004,
        1500,
        Duration::from_secs(40),
        |dice| {
            let text = damaged(&format!("# Architecture\n\n> ELI5: a map.\n\n{map}"), dice);
            let _ = council_loop::codemap::validate_map(&text, &root);
        },
    );
}

#[test]
fn readers_of_structured_input_survive_values_of_the_wrong_kind() {
    let _home = Home::enter();
    let cards = json!([
        {"id": "a", "title": "One", "body": "w ".repeat(100), "eli5": "simple", "author": "L1"},
        {"id": "b", "title": "Two", "body": "w ".repeat(100), "eli5": "plain", "authors": ["L2", "L3"]}]);
    hammer(
        "blind",
        0x5eed_0011,
        3000,
        Duration::from_secs(40),
        |dice| {
            let bent_cards = bent(&cards, dice);
            let _ = council_loop::core::blind(
                bent_cards.as_array().map(Vec::as_slice).unwrap_or(&[]),
                dice.next(),
                120,
                &[],
                1.5,
            );
        },
    );
    let votes = json!([{"voter": "L1", "ranking": ["A", "B", "C"], "best_on": {"R1": "A"}}, {"voter": "L2", "ranking": ["Candidate B", "c.", "a"], "best_on": {"R1": "C"}}]);
    let mapping = json!({"A": {"id": "a", "authors": ["L1"]}, "B": {"id": "b", "authors": ["L2", "L3"]}, "C": {"id": "c", "authors": []}});
    hammer(
        "tally",
        0x5eed_0012,
        3000,
        Duration::from_secs(40),
        |dice| {
            let (v, m, extra) = (
                bent(&votes, dice),
                bent(&mapping, dice),
                bent(&json!({"A": {"R1": 9}, "B": "fail"}), dice),
            );
            let options = council_loop::core::TallyOptions {
                scores: Some(&extra),
                checks: Some(&extra),
                keep: Some(dice.below(5)),
                ..Default::default()
            };
            let _ = council_loop::core::tally(
                v.as_array().map(Vec::as_slice).unwrap_or(&[]),
                &m,
                &options,
            );
        },
    );
    let example: Value = serde_json::from_str(include_str!(
        "../skills/council-loop/templates/run.example.json"
    ))
    .expect("the example run");
    hammer(
        "check_run",
        0x5eed_0013,
        3000,
        Duration::from_secs(40),
        |dice| {
            let mut run = example.clone();
            for _ in 0..1 + dice.below(3) {
                run = bent(&run, dice);
            }
            let _ = council_loop::core::check_run(&run);
            let _ = council_loop::core::lock_entries(&run);
            let _ = council_loop::demo::winner_title(&run);
        },
    );
    council_loop::log::set_level(council_loop::log::Level::Warn);
}

/// The first few hundred characters of a text (a report must stay readable).
fn brief(text: &str) -> String {
    let short: String = text.chars().take(300).collect();
    if short.len() < text.len() {
        format!("{short}… ({} bytes)", text.len())
    } else {
        short
    }
}

/// A value of the kind an argument asks for. Most calls must get past the argument check, so that
/// what is tested is the operation and not only its door.
fn fitting(
    param: &council_loop::api::Param,
    dice: &mut Dice,
    words: &[String],
    places: &[String],
    things: &[Value],
) -> Value {
    use council_loop::api::Kind;
    let word = |dice: &mut Dice| words[dice.below(words.len())].clone();
    match param.kind {
        Kind::Str => json!(word(dice)),
        Kind::Path => json!(places[dice.below(places.len())]),
        Kind::Choice(of) => json!(of[dice.below(of.len())]),
        Kind::Int(low, high) => {
            json!(low + (dice.next() % (high.min(low + 40) - low + 1) as u64) as i64)
        }
        Kind::Bool => json!(dice.below(2) == 0),
        Kind::StrList => json!([word(dice), word(dice)]),
        Kind::Obj | Kind::ObjList => {
            // the thing of this name when there is one (cards, votes, mapping, run), or any thing
            let named = things.iter().find_map(|t| t.get(param.name));
            let any = &things[dice.below(things.len())];
            let thing = named
                .or_else(|| any.as_object().and_then(|o| o.values().next()))
                .cloned()
                .unwrap_or(Value::Null);
            if dice.below(3) == 0 {
                bent(&thing, dice)
            } else {
                thing
            }
        }
    }
}

/// Arguments for one call: the required ones, some of the others, and now and then one of the
/// wrong kind or one that does not exist.
fn arguments(
    op: &council_loop::api::Operation,
    dice: &mut Dice,
    words: &[String],
    places: &[String],
    things: &[Value],
) -> Value {
    let mut args = serde_json::Map::new();
    for param in &op.params {
        let wanted = if param.default.is_none() {
            dice.below(25) != 0
        } else {
            dice.below(3) == 0
        };
        if !wanted {
            continue;
        }
        let value = fitting(param, dice, words, places, things);
        args.insert(
            param.name.to_string(),
            if dice.below(12) == 0 {
                bent(&value, dice)
            } else {
                value
            },
        );
    }
    if dice.below(40) == 0 {
        args.insert("no_such_argument".into(), json!(1));
    }
    Value::Object(args)
}

#[test]
fn every_operation_survives_arguments_of_every_kind() {
    use council_loop::api::{self, Context, Effect};
    let _home = Home::enter();
    let s = Sandbox::new("fuzz-api");
    s.write(
        "src/app.py",
        "import queue\n\ndef main():\n    return queue.Queue()\n",
    );
    s.write("tests/test_app.py", "def test_main():\n    assert True\n");
    s.write("notes/research/a-fact.md", "---\ntype: research\ntitle: \"Queue A keeps order within one partition only\"\nsource: \"https://example.org/a\"\n---\n");
    assert_eq!(s.run(&["lock-tests", "tests"]).code, 0);
    assert_eq!(
        s.run(&[
            "init",
            "Pick a queue for the billing pipeline",
            "--tier",
            "lite",
            "--no-research",
            "--map",
            "never"
        ])
        .code,
        0
    );
    let run_id = std::fs::read_to_string(s.proj.join(".council/runs/CURRENT"))
        .unwrap()
        .trim()
        .to_string();
    let skeleton = s.run(&["map"]);
    assert_eq!(skeleton.code, 0, "{}", skeleton.all());
    let project = s.proj.display().to_string();
    let words: Vec<String> = [
        "Pick the queue technology for the billing pipeline",
        "Rename one variable",
        "queue billing",
        "lite is fine",
        "L1",
        "L2,L3",
        "EA",
        "CTRL",
        "A",
        "B",
        &run_id,
        "no-such-run",
        "../../x",
        "solo",
        "crosslab",
        "fanout:claude",
        "fanout:nope",
        "origin/main",
        "HEAD",
        "--upload-pack=x",
        "question -> answer",
        "converged after one loop",
        "the seat timed out twice",
        "a-fact",
        "https://example.org/a",
        "test: cargo test",
        "src/app.py",
        "paper",
        "queue,billing",
        "2026-11-13",
        "",
        "\u{1b}[31mred",
        "\u{202e}reversed",
        "\0",
        "---",
        "{\"a\": 1}",
    ]
    .iter()
    .map(|w| w.to_string())
    .chain(["word ".repeat(600), "x".repeat(70_000)])
    .collect();
    let places: Vec<String> = [
        "notes",
        "docs/ARCHITECTURE.md",
        ".council/map/skeleton.md",
        "src/app.py",
        "no/such/place",
        "",
        ".",
        "..",
        "../victim",
        "~",
        "/",
        "/dev/zero",
        "/etc",
        &project,
        &s.victim.display().to_string(),
        &s.base.join("home").display().to_string(),
    ]
    .iter()
    .map(|w| w.to_string())
    .collect();
    let example: Value = serde_json::from_str(include_str!(
        "../skills/council-loop/templates/run.example.json"
    ))
    .expect("the example run");
    let things = vec![
        json!({"cards": [{"id": "a", "title": "One", "body": "w ".repeat(100), "eli5": "simple", "author": "L1"},
                         {"id": "b", "title": "Two", "body": "w ".repeat(100), "eli5": "plain", "authors": ["L2", "L3"]}]}),
        json!({"votes": [{"voter": "L1", "ranking": ["A", "B"], "best_on": {"R1": "A"}}, {"voter": "L2", "ranking": ["B", "A"]}]}),
        json!({"mapping": {"A": {"id": "a", "authors": ["L1"]}, "B": {"id": "b", "authors": ["L2", "L3"]}}}),
        json!({"scores": {"A": {"R1": 9}, "B": {"R1": 4}}}),
        json!({"checks": {"A": "pass", "B": "fail"}}),
        json!({"run": example}),
    ];
    // fenced in: the project is the default, and nothing outside the sandbox may be named
    let ctx = Context {
        project: Some(s.proj.clone()),
        roots: vec![s.base.clone()],
    };
    // operations that send prompts to model vendors are left out: a test never spends anyone's money
    let quiet: Vec<&api::Operation> = api::operations()
        .iter()
        .filter(|op| op.effect != Effect::Sends)
        .collect();
    assert!(quiet.len() >= 20, "only {} operations to try", quiet.len());
    let (mut done, mut owed, mut refused) = (0usize, 0usize, 0usize);
    let mut broke: Vec<String> = Vec::new();
    hammer("api", 0x5eed_0041, 2500, Duration::from_secs(150), |dice| {
        let op = quiet[dice.below(quiet.len())];
        let args = arguments(op, dice, &words, &places, &things);
        match api::call(op.name, &args, &ctx) {
            Ok(envelope) => {
                assert!(
                    envelope["output"].is_string() && envelope["code"].is_i64(),
                    "{}: {}",
                    op.name,
                    brief(&envelope.to_string())
                );
                assert_eq!(
                    envelope["ok"],
                    json!(envelope["code"] == 0),
                    "{}: {}",
                    op.name,
                    brief(&envelope.to_string())
                );
                if envelope["ok"] == true {
                    done += 1
                } else {
                    owed += 1
                }
            }
            Err((-32602, _)) => refused += 1,
            Err((code, why)) => broke.push(format!(
                "{} {} -> {code} {}",
                op.name,
                brief(&args.to_string()),
                brief(&why)
            )),
        }
    });
    println!("api: {done} done, {owed} answered with what is owed or wrong, {refused} refused at the door");
    assert!(
        broke.is_empty(),
        "the tool itself broke on {} call(s):\n{}",
        broke.len(),
        broke.iter().take(8).cloned().collect::<Vec<_>>().join("\n")
    );
    assert!(
        done > 250 && owed > 250,
        "the calls did not get far: {done} done, {owed} owed, {refused} refused"
    );

    // the same calls as MCP tool calls, and protocol messages bent out of shape
    let sent = std::sync::Arc::new(std::sync::Mutex::new(0usize));
    let counter = sent.clone();
    let wire: council_loop::mcp::Wire = std::sync::Arc::new(move |message: &Value| {
        assert_eq!(message["jsonrpc"], "2.0");
        *counter.lock().unwrap() += 1;
    });
    let options = council_loop::mcp::Options {
        project: Some(s.proj.clone()),
        no_dispatch: true,
        toolsets: vec!["all".to_string()],
        ..Default::default()
    };
    let mut server = council_loop::mcp::Server::with_wire(options, wire.clone());
    let messages = [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {"roots": {}}}}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        json!({"jsonrpc": "2.0", "id": 3, "method": "resources/read", "params": {"uri": "council://templates/ideate"}}),
        json!({"jsonrpc": "2.0", "id": 3, "method": "resources/read", "params": {"uri": format!("council://run/{run_id}/status")}}),
        json!({"jsonrpc": "2.0", "id": 3, "method": "resources/read", "params": {"uri": "council://run/current"}}),
        json!({"jsonrpc": "2.0", "id": 3, "method": "resources/list"}),
        json!({"jsonrpc": "2.0", "id": 4, "method": "prompts/get", "params": {"name": "council_loop", "arguments": {"task": "x y z"}}}),
        json!({"jsonrpc": "2.0", "id": 5, "result": {"roots": [{"uri": format!("file://{project}")}]}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "method": "notifications/roots/list_changed"}),
        json!({"jsonrpc": "2.0", "id": 6, "method": "logging/setLevel", "params": {"level": "error"}}),
    ];
    let all: Vec<&api::Operation> = api::operations().iter().collect();
    let (mut answered, mut errors) = (0usize, 0usize);
    hammer("mcp", 0x5eed_0042, 2500, Duration::from_secs(150), |dice| {
        let message = if dice.below(3) == 0 {
            bent(&messages[dice.below(messages.len())], dice)
        } else {
            let op = all[dice.below(all.len())];
            let mut call = json!({"jsonrpc": "2.0", "id": dice.below(1000), "method": "tools/call",
                                  "params": {"name": format!("council_{}", op.name), "arguments": arguments(op, dice, &words, &places, &things)}});
            if dice.below(4) == 0 {
                call["params"]["_meta"] = json!({"progressToken": dice.below(9)});
            }
            call
        };
        let Some(reply) = server.handle(&message) else {
            return;
        };
        assert_eq!(reply["jsonrpc"], "2.0", "{reply}");
        match (reply.get("result"), reply.get("error")) {
            (Some(result), None) => {
                answered += 1;
                if message["method"] == "tools/call" {
                    assert!(
                        result["content"].is_array(),
                        "{}",
                        brief(&reply.to_string())
                    );
                    // a refused call says so; an answered one carries the envelope
                    assert!(
                        result["isError"] == true
                            || result["structuredContent"]["output"].is_string(),
                        "{}",
                        brief(&reply.to_string())
                    );
                    let text = result["content"][0]["text"].as_str().unwrap_or_default();
                    assert!(
                        !text.contains("the tool broke"),
                        "{} -> {}",
                        brief(&message.to_string()),
                        brief(text)
                    );
                }
            }
            (None, Some(error)) => {
                errors += 1;
                assert!(
                    error["code"].is_i64() && error["message"].is_string(),
                    "{}",
                    brief(&reply.to_string())
                );
                assert_ne!(
                    error["code"],
                    -32603,
                    "the server broke: {} -> {}",
                    brief(&message.to_string()),
                    brief(&reply.to_string())
                );
            }
            _ => panic!(
                "neither a result nor an error: {}",
                brief(&reply.to_string())
            ),
        }
    });
    println!("mcp: {answered} answered, {errors} refused as protocol errors");
    assert!(
        answered > 1000,
        "the calls did not get far: {answered} answered, {errors} refused"
    );
    // a server that was given no project asks its client for the workspace: bent handshakes and
    // bent answers to its own questions (no tool is called here: there is no fence to hold them)
    let mut open = council_loop::mcp::Server::with_wire(
        council_loop::mcp::Options {
            read_only: true,
            ..Default::default()
        },
        wire.clone(),
    );
    hammer(
        "mcp-handshake",
        0x5eed_0043,
        2500,
        Duration::from_secs(60),
        |dice| {
            let message = &messages[dice.below(messages.len())];
            let message = if dice.below(2) == 0 {
                bent(message, dice)
            } else {
                message.clone()
            };
            if let Some(reply) = open.handle(&message) {
                assert_eq!(reply["jsonrpc"], "2.0");
                assert!(
                    reply.get("result").is_some() != reply.get("error").is_some(),
                    "{}",
                    brief(&reply.to_string())
                );
            }
        },
    );
    assert!(
        *sent.lock().unwrap() > 0,
        "the server never asked its client for the workspace"
    );
    council_loop::log::set_level(council_loop::log::Level::Warn);
    // the fence held: nothing outside the project, and the person's own settings were not written
    assert_eq!(
        std::fs::read_dir(&s.victim).unwrap().count(),
        0,
        "something was written outside the project"
    );
    assert!(
        !home_dir().join(".council").join("config.yaml").exists(),
        "the person's own settings were written"
    );
    assert!(!s
        .base
        .join("home")
        .join(".council")
        .join("config.yaml")
        .exists());
}

#[test]
fn the_hook_survives_any_event() {
    let _home = Home::enter();
    let s = Sandbox::new("fuzz-hook");
    s.write("tests/test_a.py", "def test_a(): pass\n");
    assert_eq!(s.run(&["lock-tests", "tests"]).code, 0);
    let cwd = s.proj.display().to_string();
    let events = [
        json!({"hook_event_name": "PreToolUse", "tool_name": "Edit", "cwd": cwd, "tool_input": {"file_path": "tests/test_a.py"}}),
        json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "cwd": cwd, "tool_input": {"command": "cd tests && rm -f 'a b.py' x.py; echo done > out.log 2>&1 | tee -a log"}}),
        json!({"hook_event_name": "PreToolUse", "agent_type": "council-loop:council-researcher", "tool_name": "Read", "cwd": cwd, "tool_input": {"file_path": "src/app.py"}}),
        json!({"hook_event_name": "PreToolUse", "agent_type": "council-expert", "tool_name": "Write", "cwd": cwd, "tool_input": {"file_path": ".council/runs/r/loop-1/ideate/L1.out.json", "content": "{}"}}),
        json!({"hook_event_name": "PostToolUse", "tool_name": "Bash", "cwd": cwd, "tool_input": {"command": "make test"}, "tool_response": {"stdout": "ok"}}),
    ];
    // in this process: the working directory does not matter, the event names the project
    hammer("hook", 0x5eed_0021, 4000, Duration::from_secs(60), |dice| {
        let event = &events[dice.below(events.len())];
        let raw = if dice.below(2) == 0 {
            bent(event, dice).to_string()
        } else {
            damaged(&event.to_string(), dice)
        };
        let _ = council_loop::hook::decide(raw.as_bytes());
    });
    // shell lines of every shape
    hammer(
        "hook-shell",
        0x5eed_0022,
        4000,
        Duration::from_secs(60),
        |dice| {
            let line = damaged("cd src && FOO=1 sudo rm -rf \"tests/a b\" ./x > out.txt 2>&1 || cp -r a b | tee $(cat list) <<EOF\nx\nEOF", dice);
            let event = json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "cwd": cwd, "tool_input": {"command": line}});
            let _ = council_loop::hook::decide(event.to_string().as_bytes());
        },
    );
}

#[test]
fn the_run_loop_survives_damaged_state_and_damaged_answers() {
    use council_loop::demo::{script, Stage};
    let s = Sandbox::new("fuzz-run");
    let stage = Stage::new(&s.proj, &s.base.join("own"), &common::council())
        .unwrap()
        .with_env("CI", None);
    let run = |args: &[&str]| stage.council(args).unwrap();
    assert_eq!(
        run(&[
            "init",
            council_loop::demo::ASK,
            "--tier",
            "lite",
            "--mode",
            "fanout:claude",
            "--no-research",
            "--map",
            "never"
        ])
        .code,
        0
    );
    let d = stage.run_dir().unwrap();
    std::fs::write(d.join("brief.yaml"), script::brief(&stage.run().unwrap())).unwrap();
    assert_eq!(run(&["ingest"]).code, 0);
    let good_state = std::fs::read_to_string(d.join("run.json")).unwrap();
    let state: Value = serde_json::from_str(&good_state).unwrap();
    let ideate = d.join("loop-1").join("ideate");
    let answers = seat_outputs();
    let mut dice = Dice(0x5eed_0031);
    let started = Instant::now();
    let no_crash = |what: &str, out: &council_loop::demo::Played| {
        assert!(
            out.code == 0 || out.code == 1 || out.code == 2,
            "{what}: exit code {}\n{}{}",
            out.code,
            out.out,
            out.err
        );
        assert!(!out.err.contains("panicked"), "{what}:\n{}", out.err);
    };
    // the state file, bent and torn: every verb answers and never writes outside
    for round in 0..40 {
        let text = if round % 2 == 0 {
            bent(&state, &mut dice).to_string()
        } else {
            damaged(&good_state, &mut dice)
        };
        std::fs::write(d.join("run.json"), text).unwrap();
        for verb in [
            &["status"][..],
            &["next"],
            &["ingest"],
            &["audit"],
            &["log"],
            &["check"],
            &["dispatch", "--dry-run"],
        ] {
            no_crash(&format!("round {round} {verb:?}"), &run(verb));
        }
    }
    std::fs::write(d.join("run.json"), &good_state).unwrap();
    // the seats' answers, bent and torn: rejected with a reason, or accepted, never a crash
    for round in 0..40 {
        for seat in ["L1", "L2", "L3"] {
            let answer = &answers[dice.below(answers.len())];
            let text = if round % 2 == 0 {
                bent(answer, &mut dice).to_string()
            } else {
                damaged(&answer.to_string(), &mut dice)
            };
            std::fs::write(ideate.join(format!("{seat}.out.json")), text).unwrap();
        }
        no_crash(&format!("answers, round {round}"), &run(&["ingest"]));
        std::fs::write(d.join("run.json"), &good_state).unwrap();
        for entry in std::fs::read_dir(&ideate).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.contains(".rejected-") || name.ends_with(".retry.md") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    assert!(
        started.elapsed() < Duration::from_secs(120),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(std::fs::read_dir(&s.victim).unwrap().count(), 0);
}
