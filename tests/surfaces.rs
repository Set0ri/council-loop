//! The surfaces other programs meet: the API (`council api`), the MCP server, the hook, and what
//! the command line says about itself (the first-run check, the demo, the diary, diagnostics).
//! Everything here goes through the real binary.

mod common;

use common::{finish, Sandbox};
use council_loop::demo::script;
use serde_json::{json, Value};
use std::io::{BufRead as _, BufReader, Write as _};
use std::process::{Child, ChildStdin, ChildStdout, Stdio};

const ASK: &str = "Pick the queue technology for the billing pipeline";

// ----------------------------------------------------------------------------- a small MCP client

struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next: u64,
    /// Notifications and requests the server sent on its own.
    unasked: Vec<Value>,
}

impl Client {
    fn start(s: &Sandbox, args: &[&str]) -> Client {
        let mut all = vec!["mcp"];
        all.extend_from_slice(args);
        let mut child = s
            .command(&all)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the server starts");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        Client {
            child,
            stdin,
            stdout,
            next: 1,
            unasked: Vec::new(),
        }
    }

    fn send(&mut self, message: &Value) {
        writeln!(self.stdin, "{message}").expect("the server reads");
        self.stdin.flush().expect("flush");
    }

    fn read(&mut self) -> Value {
        let mut line = String::new();
        self.stdout
            .read_line(&mut line)
            .expect("the server answers");
        serde_json::from_str(&line)
            .unwrap_or_else(|e| panic!("not a protocol message ({e}): {line:?}"))
    }

    /// Send a request and return its result (or its error object).
    fn ask(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let message = self.read();
            if message.get("method").is_none() && message["id"] == json!(id) {
                return message
                    .get("result")
                    .cloned()
                    .unwrap_or_else(|| message["error"].clone());
            }
            self.unasked.push(message);
        }
    }

    fn tool(&mut self, name: &str, arguments: Value) -> Value {
        self.ask("tools/call", json!({"name": name, "arguments": arguments}))
    }

    fn hello(&mut self, capabilities: Value) -> Value {
        let reply = self.ask("initialize", json!({"protocolVersion": "2025-06-18", "capabilities": capabilities, "clientInfo": {"name": "test", "version": "0"}}));
        self.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        reply
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ----------------------------------------------------------------------------- the MCP server

#[test]
fn a_whole_council_can_be_run_through_the_mcp_tools_from_their_structured_answers_alone() {
    let s = Sandbox::new("mcp-run");
    council_loop::demo::write_project(&s.proj).unwrap();
    let mut mcp = Client::start(&s, &["--project", &s.proj.display().to_string()]);
    let hello = mcp.hello(json!({}));
    assert_eq!(hello["serverInfo"]["name"], "council-loop");
    assert!(hello["instructions"]
        .as_str()
        .unwrap()
        .contains("council_init"));
    let plan = mcp.tool("council_plan", json!({"task": ASK, "tier": "lite"}));
    assert_eq!(plan["structuredContent"]["data"]["tier"], "lite");
    assert_eq!(
        plan["structuredContent"]["data"]["chosen_by"],
        json!({"mode": "default", "profile": "default"})
    );
    let mut state = mcp.tool(
        "council_init",
        json!({"ask": ASK, "tier": "lite", "mode": "fanout:claude"}),
    );
    assert_eq!(state["isError"], false, "{state}");
    let mut steps = 0;
    // no text is parsed below: the loop is driven by `run.phase` and `run.owed`
    loop {
        let run = state["structuredContent"]["run"].clone();
        assert!(run.is_object(), "{state}");
        steps += 1;
        assert!(steps < 60, "the run does not move: {run}");
        let dir = std::path::PathBuf::from(run["dir"].as_str().unwrap());
        let at = run["loop"].as_i64().unwrap();
        let loop_dir = dir.join(format!("loop-{at}"));
        match run["phase"].as_str().unwrap() {
            "done" => break,
            "brief" => {
                let whole: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("run.json")).unwrap()).unwrap();
                std::fs::write(dir.join("brief.yaml"), script::brief(&whole)).unwrap();
            }
            "merge" => script::approve_pool(&loop_dir.join("pool.json")).unwrap(),
            "fuse" => script::fill_fuse(&loop_dir.join("candidates.json")).unwrap(),
            "cut" => {
                state = mcp.tool("council_cut", json!({"eli5": "one plan won on reliability"}));
                assert_eq!(state["structuredContent"]["ok"], true, "{state}");
                continue;
            }
            "final" => script::fill_todos(&dir.join("final.md"), &[], "\nDecided unattended where noted. Stand-in seats are labelled. The control baseline was read.\n").unwrap(),
            "retro" => script::fill_todos(&dir.join("retro.md"), &[], "").unwrap(),
            _ => {
                let owed = run["owed"].as_array().unwrap();
                if owed.is_empty() {
                    state = mcp.tool("council_next", json!({}));
                    continue;
                }
                for seat in owed.iter().filter(|seat| seat["state"] == "missing") {
                    let packet = std::fs::read_to_string(seat["prompt"].as_str().unwrap()).unwrap();
                    std::fs::write(seat["output"].as_str().unwrap(), script::answer(&packet).unwrap()).unwrap();
                }
            }
        }
        state = mcp.tool("council_ingest", json!({}));
        assert_eq!(state["structuredContent"]["ok"], true, "{state}");
    }
    assert_eq!(
        state["structuredContent"]["run"]["next"],
        "nothing: the run is finished"
    );
    let audit = mcp.tool("council_audit", json!({}));
    assert!(
        audit["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("audit: ok"),
        "{audit}"
    );
    // what there is to read afterwards
    let listed = mcp.ask("resources/list", json!({}));
    let uris: Vec<&str> = listed["resources"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["uri"].as_str())
        .collect();
    assert!(
        uris.contains(&"council://run/current") && uris.contains(&"council://map"),
        "{uris:?}"
    );
    let read = |mcp: &mut Client, uri: &str| {
        mcp.ask("resources/read", json!({"uri": uri}))["contents"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string()
    };
    let runs: Value = serde_json::from_str(&read(&mut mcp, "council://runs")).unwrap();
    let id = runs["runs"][0]["id"].as_str().unwrap().to_string();
    assert_eq!(runs["runs"][0]["phase"], "done");
    assert!(read(&mut mcp, &format!("council://run/{id}/final")).contains("## Why this wins"));
    assert!(read(&mut mcp, &format!("council://run/{id}/ledger/1")).contains("Loop 1"));
    assert!(read(&mut mcp, &format!("council://run/{id}/events")).contains("\"verb\": \"cut\""));
    assert!(read(&mut mcp, "council://run/current").contains("phase done"));
    assert!(read(&mut mcp, "council://map").contains("## Code map"));
    assert!(read(&mut mcp, "council://memory/index").contains("idempotency"));
    assert_eq!(
        mcp.ask("resources/read", json!({"uri": "council://run/nope/final"}))["code"],
        -32002
    );
    let resume = mcp.ask("prompts/get", json!({"name": "council_resume"}));
    assert!(resume["messages"][0]["content"]["text"]
        .as_str()
        .unwrap()
        .contains("phase done"));
}

#[test]
fn the_server_follows_the_clients_workspace_and_tells_it_what_it_does() {
    let s = Sandbox::new("mcp-roots");
    let mut mcp = Client::start(&s, &["--toolsets", "all"]);
    mcp.hello(json!({"roots": {"listChanged": true}}));
    // before the roots are known there is no project
    let early = mcp.tool("council_memory_status", json!({}));
    let asked = mcp
        .unasked
        .iter()
        .find(|m| m["method"] == "roots/list")
        .cloned()
        .expect("the server asks for the roots");
    assert_eq!(early["isError"], true, "{early}");
    let uri = format!(
        "file://{}{}",
        if cfg!(windows) { "/" } else { "" },
        s.proj.display().to_string().replace('\\', "/")
    );
    mcp.send(&json!({"jsonrpc": "2.0", "id": asked["id"], "result": {"roots": [{"uri": uri, "name": "proj"}]}}));
    let status = mcp.tool("council_memory_status", json!({}));
    assert_eq!(status["structuredContent"]["ok"], true, "{status}");
    assert!(status["structuredContent"]["data"]["dir"]
        .as_str()
        .unwrap()
        .starts_with(&s.proj.display().to_string()));
    // a project outside the client's workspace is refused
    let outside = mcp.tool(
        "council_memory_status",
        json!({"project_dir": s.victim.display().to_string()}),
    );
    assert_eq!(outside["isError"], true);
    // logging: quiet by default, talkative when asked
    assert_eq!(
        mcp.ask("logging/setLevel", json!({"level": "info"})),
        json!({})
    );
    let plan = mcp.tool("council_plan", json!({"task": ASK}));
    assert_eq!(plan["structuredContent"]["ok"], true);
    let said = mcp
        .unasked
        .iter()
        .find(|m| m["method"] == "notifications/message")
        .cloned()
        .expect("a log message");
    assert_eq!(said["params"]["level"], "info");
    assert_eq!(said["params"]["logger"], "council.gate");
    // documents: as resources, and as a tool for clients that only call tools
    let listed = mcp.ask("resources/list", json!({}));
    assert!(
        listed["resources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["uri"] == "council://run/current"),
        "{listed}"
    );
    let templates = mcp.ask("resources/templates/list", json!({}));
    assert!(templates["resourceTemplates"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["uriTemplate"] == "council://run/{id}/ledger/{loop}"));
    let config = mcp.ask("resources/read", json!({"uri": "council://config"}));
    assert!(
        config["contents"][0]["text"]
            .as_str()
            .unwrap()
            .contains("memory"),
        "{config}"
    );
    let skill = mcp.tool(
        "council_read",
        json!({"uri": "skill://council-loop/SKILL.md"}),
    );
    assert!(skill["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("name: council-loop"));
    assert_eq!(
        skill["structuredContent"]["data"]["mime_type"],
        "text/markdown"
    );
    // the prompt's gate is sized for the client's project
    let prompt = mcp.ask(
        "prompts/get",
        json!({"name": "council_loop", "arguments": {"task": ASK}}),
    );
    assert!(
        prompt["messages"][0]["content"]["text"]
            .as_str()
            .unwrap()
            .contains("\nGATE tier="),
        "{prompt}"
    );
    // a pure operation needs no project; its data is also in the text for clients that read only text
    let models = mcp.tool("council_models", json!({"mode": "fanout:claude"}));
    assert!(models["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("\"relative_cost\""));
    // every listed tool has a schema that names its arguments and refuses unknown ones
    let tools = mcp.ask("tools/list", json!({}));
    for tool in tools["tools"].as_array().unwrap() {
        assert_eq!(
            tool["inputSchema"]["additionalProperties"], false,
            "{}",
            tool["name"]
        );
        assert!(tool["description"].as_str().unwrap().len() > 30);
    }
}

#[test]
fn progress_is_reported_while_seats_are_dispatched() {
    // the seats sit behind `claude`, which here is a two-line script that answers nothing useful:
    // enough to see one progress message per seat
    if cfg!(windows) {
        return;
    }
    let s = Sandbox::new("mcp-progress");
    let tools = s.base.join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    std::fs::write(
        tools.join("claude"),
        "#!/bin/sh\ncat > /dev/null\necho '{\"lens\": \"x\"}'\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(tools.join("claude"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
    }
    let path = format!(
        "{}:{}",
        tools.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut cmd = s.command(&["mcp", "--project", &s.proj.display().to_string()]);
    cmd.env("PATH", &path)
        .env_remove("CLAUDECODE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().unwrap();
    let mut mcp = Client {
        stdin: child.stdin.take().unwrap(),
        stdout: BufReader::new(child.stdout.take().unwrap()),
        child,
        next: 1,
        unasked: Vec::new(),
    };
    mcp.hello(json!({}));
    let init = mcp.tool("council_init", json!({"ask": ASK, "tier": "lite", "mode": "fanout:claude", "research": 0, "mapper": "never"}));
    assert_eq!(init["structuredContent"]["ok"], true, "{init}");
    let dir = std::path::PathBuf::from(init["structuredContent"]["run"]["dir"].as_str().unwrap());
    let whole: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("run.json")).unwrap()).unwrap();
    std::fs::write(dir.join("brief.yaml"), script::brief(&whole)).unwrap();
    assert_eq!(
        mcp.tool("council_ingest", json!({}))["structuredContent"]["ok"],
        true
    );
    let sent = mcp.ask(
        "tools/call",
        json!({"name": "council_dispatch", "arguments": {}, "_meta": {"progressToken": "p1"}}),
    );
    assert_eq!(sent["structuredContent"]["ok"], true, "{sent}");
    let progress: Vec<&Value> = mcp
        .unasked
        .iter()
        .filter(|m| m["method"] == "notifications/progress")
        .collect();
    assert_eq!(progress.len(), 4, "{:?}", mcp.unasked); // three experts and the control arm
    assert!(progress
        .iter()
        .all(|p| p["params"]["progressToken"] == "p1"));
    assert!(progress[0]["params"]["message"]
        .as_str()
        .unwrap()
        .starts_with("seat "));
}

#[test]
fn a_server_can_be_narrowed_by_flags_the_environment_and_your_own_settings() {
    let s = Sandbox::new("mcp-options");
    let names = |args: &[&str], env: &[(&str, &str)]| -> Vec<String> {
        let mut all = vec!["mcp"];
        all.extend_from_slice(args);
        let out = s.run_with(
            &all,
            env,
            Some("{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n"),
        );
        let reply: Value =
            serde_json::from_str(out.out.lines().next().unwrap_or("null")).unwrap_or(Value::Null);
        reply["result"]["tools"]
            .as_array()
            .map(|t| {
                t.iter()
                    .map(|x| x["name"].as_str().unwrap().to_string())
                    .collect()
            })
            .unwrap_or_default()
    };
    let default = names(&[], &[]);
    assert_eq!(default.len(), 22);
    assert!(
        !default.contains(&"council_tally".to_string())
            && default.contains(&"council_read".to_string())
    );
    assert_eq!(names(&["--toolsets", "all"], &[]).len(), 27);
    assert_eq!(names(&[], &[("COUNCIL_MCP_TOOLSETS", "memory")]).len(), 6);
    assert_eq!(
        names(&["--tools", "status,council_next"], &[]),
        ["council_next", "council_status"]
    );
    let read_only = names(&["--read-only"], &[]);
    assert!(
        read_only.contains(&"council_status".to_string())
            && !read_only.contains(&"council_ingest".to_string())
    );
    assert!(
        !names(&[], &[("COUNCIL_MCP_NO_DISPATCH", "1")]).contains(&"council_dispatch".to_string())
    );
    // your own settings file; a flag still beats it
    std::fs::create_dir_all(s.base.join("home").join(".council")).unwrap();
    std::fs::write(
        s.base.join("home").join(".council").join("config.yaml"),
        "mcp:\n  read_only: true\n  toolsets: [run]\n  deny_tools: [audit]\n",
    )
    .unwrap();
    let own = names(&[], &[]);
    assert!(
        own.contains(&"council_status".to_string())
            && !own.contains(&"council_audit".to_string())
            && !own.contains(&"council_init".to_string()),
        "{own:?}"
    );
    assert!(names(&["--toolsets", "memory"], &[]).contains(&"council_memory_search".to_string()));
    // a name that is not one is an error at the start, not a silently different server
    let bad = s.run_with(&["mcp", "--tools", "statsu"], &[], Some(""));
    assert_eq!(bad.code, 2);
    assert!(
        bad.err.contains("no operation named statsu"),
        "{}",
        bad.all()
    );
    assert_eq!(
        s.run_with(&["mcp", "--toolsets", "everything"], &[], Some(""))
            .code,
        2
    );
    assert_eq!(
        s.run_with(&["mcp", "--project", "relative/dir"], &[], Some(""))
            .code,
        2
    );
}

// ----------------------------------------------------------------------------- the API from a shell

#[test]
fn the_api_is_listed_described_and_called_from_a_shell() {
    let s = Sandbox::new("api");
    let listed = s.run(&["api", "--list"]);
    assert_eq!(listed.code, 0);
    assert!(
        listed.out.contains("27 operations") && listed.out.contains("memory_search   reads"),
        "{}",
        listed.out
    );
    let described: Value = serde_json::from_str(&s.run(&["api", "--describe"]).out).unwrap();
    assert_eq!(described["api_version"], "1");
    assert_eq!(described["operations"].as_array().unwrap().len(), 27);
    assert!(described["documents"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["uri"] == "council://run/{id}/final"));
    let one: Value = serde_json::from_str(&s.run(&["api", "init", "--describe"]).out).unwrap();
    assert_eq!(one["mcp_tool"], "council_init");
    assert_eq!(one["input"]["required"], json!(["ask"]));
    assert!(one["output"]["properties"]["run"]["properties"]
        .get("owed")
        .is_some());
    // a call: the working directory is the project; the exit code is the envelope's code
    let plan = s.run(&[
        "api",
        "plan",
        "--args",
        &json!({"task": ASK, "stakes": "medium"}).to_string(),
    ]);
    assert_eq!(plan.code, 0, "{}", plan.all());
    let envelope: Value = serde_json::from_str(&plan.out).unwrap();
    assert_eq!(
        (envelope["ok"].clone(), envelope["data"]["tier"].clone()),
        (json!(true), json!("lite"))
    );
    assert!(envelope["output"]
        .as_str()
        .unwrap()
        .starts_with("GATE tier=lite"));
    let started = s.run_with(
        &["api", "init", "--args-file", "-"],
        &[],
        Some(&json!({"ask": ASK, "tier": "lite", "mode": "fanout:claude"}).to_string()),
    );
    assert_eq!(started.code, 0, "{}", started.all());
    let envelope: Value = serde_json::from_str(&started.out).unwrap();
    assert_eq!(envelope["run"]["phase"], "brief");
    assert!(envelope["run"]["next"]
        .as_str()
        .unwrap()
        .contains("brief.yaml"));
    // documents come through the same door: the skill, and what the project holds
    let skill = s.run(&[
        "api",
        "read",
        "--args",
        &json!({"uri": "skill://council-loop/SKILL.md"}).to_string(),
    ]);
    assert_eq!(skill.code, 0, "{}", skill.all());
    let envelope: Value = serde_json::from_str(&skill.out).unwrap();
    assert!(envelope["output"]
        .as_str()
        .unwrap()
        .contains("name: council-loop"));
    assert_eq!(
        envelope["data"],
        json!({"uri": "skill://council-loop/SKILL.md", "mime_type": "text/markdown"})
    );
    let current = s.run(&[
        "api",
        "read",
        "--args",
        &json!({"uri": "council://run/current"}).to_string(),
    ]);
    let envelope: Value = serde_json::from_str(&current.out).unwrap();
    assert!(
        envelope["output"]
            .as_str()
            .unwrap()
            .starts_with("GATE tier=lite"),
        "{}",
        current.all()
    );
    let runs = s.run(&[
        "api",
        "read",
        "--args",
        &json!({"uri": "council://runs"}).to_string(),
    ]);
    let envelope: Value = serde_json::from_str(&runs.out).unwrap();
    let runs: Value = serde_json::from_str(envelope["output"].as_str().unwrap()).unwrap();
    assert_eq!(runs["runs"].as_array().unwrap().len(), 1);
    let nothing = s.run(&[
        "api",
        "read",
        "--args",
        &json!({"uri": "council://map"}).to_string(),
    ]);
    assert_eq!(nothing.code, 2);
    assert!(
        nothing.out.contains("Nothing to read at council://map yet"),
        "{}",
        nothing.all()
    );
    let stuck = s.run(&["api", "ingest"]);
    assert_eq!(stuck.code, 2, "{}", stuck.all());
    let envelope: Value = serde_json::from_str(&stuck.out).unwrap();
    assert_eq!(envelope["ok"], false);
    assert!(envelope["output"]
        .as_str()
        .unwrap()
        .contains("brief.yaml is not ready"));
    assert_eq!(envelope["run"]["phase"], "brief");
    // refused before anything runs
    for (args, why) in [
        (vec!["api", "nope"], "Unknown operation: nope"),
        (
            vec!["api", "status", "--args", "{\"run\": 1}"],
            "run: no such argument",
        ),
        (
            vec!["api", "status", "--args", "[1]"],
            "the arguments must be an object",
        ),
        (
            vec!["api", "status", "--args", "{not json"],
            "not valid JSON",
        ),
    ] {
        let out = s.run(&args);
        assert_eq!(out.code, 2, "{args:?}: {}", out.all());
        assert!(out.err.contains(why), "{args:?}: {}", out.all());
    }
}

// ----------------------------------------------------------------------------- the hook

fn hook(s: &Sandbox, event: Value, env: &[(&str, &str)]) -> (i32, Value) {
    let out = s.run_with(&["hook"], env, Some(&event.to_string()));
    (
        out.code,
        serde_json::from_str(out.out.trim()).unwrap_or(Value::Null),
    )
}

#[test]
fn the_hook_guards_locked_files_before_and_after_a_tool_runs() {
    let s = Sandbox::new("hook-locks");
    s.write("tests/test_a.py", "def test_a():\n    assert 1 == 1\n");
    s.write("src/app.py", "x = 1\n");
    let cwd = s.proj.display().to_string();
    let event = |name: &str, tool: &str, input: Value| json!({"hook_event_name": name, "tool_name": tool, "cwd": cwd, "tool_input": input});
    // nothing is locked: nothing is said
    assert_eq!(
        hook(
            &s,
            event(
                "PreToolUse",
                "Edit",
                json!({"file_path": "tests/test_a.py"})
            ),
            &[]
        ),
        (0, Value::Null)
    );
    assert_eq!(s.run(&["lock-tests", "tests"]).code, 0);
    let denied = hook(
        &s,
        event(
            "PreToolUse",
            "Edit",
            json!({"file_path": s.proj.join("tests").join("test_a.py").display().to_string()}),
        ),
        &[],
    )
    .1;
    assert_eq!(denied["hookSpecificOutput"]["permissionDecision"], "deny");
    assert!(denied["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap()
        .contains("tests/test_a.py is locked"));
    for command in [
        "rm -rf tests",
        "cd tests && sed -i s/1/2/ test_a.py",
        "echo x > tests/test_a.py",
        "council lock-tests tests",
    ] {
        let denied = hook(
            &s,
            event("PreToolUse", "Bash", json!({"command": command})),
            &[],
        )
        .1;
        assert_eq!(
            denied["hookSpecificOutput"]["permissionDecision"], "deny",
            "{command}"
        );
    }
    for command in [
        "pytest -q tests/test_a.py 2>&1 | tail -5",
        "cat tests/test_a.py",
        "rm src/app.py",
        "git checkout -- tests/test_a.py",
    ] {
        assert_eq!(
            hook(
                &s,
                event("PreToolUse", "Bash", json!({"command": command})),
                &[]
            ),
            (0, Value::Null),
            "{command}"
        );
    }
    assert_eq!(
        hook(
            &s,
            event("PreToolUse", "Edit", json!({"file_path": "src/app.py"})),
            &[]
        ),
        (0, Value::Null)
    );
    // the owner unlocks for a session
    assert_eq!(
        hook(
            &s,
            event(
                "PreToolUse",
                "Edit",
                json!({"file_path": "tests/test_a.py"})
            ),
            &[("COUNCIL_ALLOW_LOCKED", "1")]
        ),
        (0, Value::Null)
    );
    // after a command ran: silent while the files match, one objection when one does not
    let after = event(
        "PostToolUse",
        "Bash",
        json!({"command": "python tools/regen.py"}),
    );
    assert_eq!(hook(&s, after.clone(), &[]), (0, Value::Null));
    s.write(
        "tests/test_a.py",
        "def test_a():\n    assert True  # made to pass by a script\n",
    );
    let objection = hook(&s, after.clone(), &[]).1;
    assert_eq!(objection["decision"], "block");
    assert!(
        objection["reason"]
            .as_str()
            .unwrap()
            .contains("tests/test_a.py"),
        "{objection}"
    );
    assert_eq!(hook(&s, after.clone(), &[]), (0, Value::Null)); // said once
    s.write("tests/test_a.py", "def test_a():\n    assert 1 == 1\n");
    assert_eq!(hook(&s, after, &[]), (0, Value::Null));
    // rubbish on standard input, an event of another kind: exit 0 and silence, always
    for raw in ["", "not json", "[1,2]", "{\"hook_event_name\": \"Stop\"}"] {
        let out = s.run_with(&["hook"], &[], Some(raw));
        assert_eq!((out.code, out.out.as_str()), (0, ""), "{raw:?}");
    }
}

#[test]
fn the_hook_keeps_council_seats_in_their_lane() {
    let s = Sandbox::new("hook-seats");
    let run = s
        .proj
        .join(".council")
        .join("runs")
        .join("r1")
        .join("loop-1")
        .join("vote");
    std::fs::create_dir_all(&run).unwrap();
    s.write("src/app.py", "x = 1\n");
    let cwd = s.proj.display().to_string();
    let call = |agent: &str, tool: &str, key: &str, path: &std::path::Path| {
        hook(&s, json!({"hook_event_name": "PreToolUse", "agent_type": agent, "tool_name": tool, "cwd": cwd,
                        "tool_input": {key: path.display().to_string()}}), &[]).1
    };
    let denied = |answer: &Value| answer["hookSpecificOutput"]["permissionDecision"] == "deny";
    assert!(!denied(&call(
        "council-loop:council-expert",
        "Read",
        "file_path",
        &run.join("L1.prompt.md")
    )));
    assert!(!denied(&call(
        "council-loop:council-expert",
        "Write",
        "file_path",
        &run.join("L1.out.json")
    )));
    assert!(denied(&call(
        "council-loop:council-expert",
        "Read",
        "file_path",
        &run.join("L2.out.json")
    )));
    assert!(denied(&call(
        "council-loop:council-expert",
        "Edit",
        "file_path",
        &s.proj.join("src").join("app.py")
    )));
    assert!(denied(&call(
        "council-elder",
        "Write",
        "file_path",
        &s.proj
            .join(".council")
            .join("runs")
            .join("r1")
            .join("run.json")
    )));
    assert!(denied(&call(
        "council-researcher",
        "Read",
        "file_path",
        &s.proj.join("src").join("app.py")
    )));
    assert!(denied(&call(
        "council-expert",
        "Read",
        "file_path",
        &s.base.join("private").join("r1.json")
    )));
    // the chair and everyone else's agents are not seats
    assert!(!denied(&call(
        "",
        "Edit",
        "file_path",
        &s.proj.join("src").join("app.py")
    )));
    assert!(!denied(&call(
        "code-reviewer",
        "Read",
        "file_path",
        &run.join("L2.out.json")
    )));
}

// ----------------------------------------------------------------------------- what the tool says about itself

#[test]
fn the_first_run_check_is_said_once_and_only_to_people() {
    let s = Sandbox::new("welcome");
    let run = |args: &[&str], env: &[(&str, &str)]| {
        let mut cmd = s.command(args);
        cmd.env_remove("COUNCIL_NO_WELCOME");
        for (name, value) in env {
            cmd.env(name, value);
        }
        finish(cmd, Some(""))
    };
    let marker = s.base.join("home").join(".council").join("install.json");
    // programs are not greeted, and CI is not
    for args in [&["hook"][..], &["api", "--list"], &["--version"]] {
        assert!(!run(args, &[]).err.contains("first run"), "{args:?}");
    }
    assert!(!run(&["models"], &[("CI", "true")])
        .err
        .contains("first run"));
    assert!(!marker.exists());
    let first = run(&["models"], &[]);
    assert_eq!(first.code, 0);
    assert!(
        first.err.starts_with(&format!(
            "council {} · first run on this machine",
            env!("CARGO_PKG_VERSION")
        )),
        "{}",
        first.err
    );
    for line in [
        "[ok] seats can start: as subagents",
        "model data:",
        "a run here defaults to mode=fanout:claude profile=efficient",
        "council demo",
        "council doctor",
    ] {
        assert!(first.err.contains(line), "{line}: {}", first.err);
    }
    assert!(
        serde_json::from_str::<Value>(&first.out).is_ok(),
        "the command's own output stays clean"
    );
    assert!(marker.exists());
    assert_eq!(run(&["models"], &[]).err, "");
    // after an update: one line
    std::fs::write(
        &marker,
        "{\"version\": \"0.1.0\", \"first_run\": \"2026-01-01\"}",
    )
    .unwrap();
    let updated = run(&["models"], &[]);
    assert!(
        updated.err.starts_with("council was updated: 0.1.0 -> "),
        "{}",
        updated.err
    );
    assert_eq!(updated.err.lines().count(), 1);
    assert_eq!(run(&["models"], &[]).err, "");
    assert!(std::fs::read_to_string(&marker)
        .unwrap()
        .contains("\"first_run\": \"2026-01-01\""));
}

#[test]
fn the_demo_plays_a_whole_council_and_leaves_your_settings_alone() {
    let s = Sandbox::new("demo");
    let kept = s.base.join("kept");
    let out = s.run(&["demo", "--dir", &kept.display().to_string()]);
    assert_eq!(out.code, 0, "{}", out.all());
    for line in [
        "No model is called",
        "$ council init",
        "$ council cut",
        "the decision:  Managed queue with idempotency keys",
        "the audit:     audit: ok",
        "council init \"<the ask",
    ] {
        assert!(out.out.contains(line), "{line}: {}", out.out);
    }
    assert!(kept.join("docs").join("ARCHITECTURE.md").is_file());
    // nothing of the demo lands in the settings or the private folder of the person who ran it
    assert!(!s
        .base
        .join("home")
        .join(".council")
        .join("config.yaml")
        .exists());
    assert!(!s.base.join("private").exists());
    // the kept folder is a finished run like any other
    let mut cmd = s.command(&["log", "-n", "3"]);
    cmd.current_dir(&kept);
    let log = finish(cmd, None);
    assert_eq!(log.code, 0, "{}", log.all());
    assert!(log.out.contains("retro -> done"), "{}", log.out);
    // a folder that holds something is not played in
    assert_eq!(
        s.run(&["demo", "--dir", &kept.display().to_string()]).code,
        2
    );
    // the default: a folder of its own, removed afterwards
    let out = s.run(&["demo"]);
    assert_eq!(out.code, 0, "{}", out.all());
    assert!(out.out.contains("the folder was removed"));
}

#[test]
fn diagnostics_are_asked_for_and_never_mix_with_a_commands_answer() {
    let s = Sandbox::new("diagnostics");
    let quiet = s.run(&["plan", ASK]);
    assert_eq!(quiet.err, "");
    let chatty = s.run(&["-v", "plan", ASK]);
    assert_eq!(chatty.out, quiet.out);
    assert!(
        chatty.err.contains("[council info gate] tier=lite"),
        "{}",
        chatty.err
    );
    let as_json = s.run_with(
        &["plan", ASK],
        &[("COUNCIL_LOG", "info"), ("COUNCIL_LOG_FORMAT", "json")],
        None,
    );
    let line: Value = serde_json::from_str(as_json.err.lines().next().unwrap_or("")).unwrap();
    assert_eq!(
        (line["level"].clone(), line["target"].clone()),
        (json!("info"), json!("gate"))
    );
    assert_eq!(s.run(&["--log-level", "off", "-v", "plan", ASK]).err, "");
    // no command: the menu, and success (a hook runner that drops the argument must not block every tool call)
    let bare = s.run(&[]);
    assert_eq!(bare.code, 0);
    assert!(bare.out.contains("demo") && bare.out.contains("verify-locks"));
    assert_eq!(
        s.run(&["--version"]).out.trim(),
        format!("council {}", env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(s.run(&["no-such-verb"]).code, 2);
    // a free-text value may start with a hyphen
    let out = s.run(&[
        "memory",
        "add",
        "--claim",
        "Queue A keeps order within one partition only",
        "--source",
        "https://example.org/a",
        "--why",
        "--- it bounds the design",
    ]);
    assert_eq!(out.code, 0, "{}", out.all());
}
