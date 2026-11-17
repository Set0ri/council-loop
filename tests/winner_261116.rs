//! Acceptance tests for the 261116 Council Loop build step.
//! Tests are written first by the independent Test Author and locked before implementation.

mod common;

use common::Sandbox;
use serde_json::{json, Value};
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::process::{Child, Stdio};
use std::time::Duration;

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn mcp_list_tools(s: &Sandbox, extra_args: &[&str]) -> Value {
    let mut args = vec!["mcp", "--toolsets", "all"];
    args.extend_from_slice(extra_args);
    let mut cmd = s.command(&args);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().expect("spawn council mcp");
    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout"));
    let _guard = ChildGuard(child);

    // 1. send initialize
    let init_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "test-compact", "version": "1.0"}
        }
    });
    writeln!(stdin, "{init_req}").expect("write initialize");
    stdin.flush().expect("flush initialize");

    let mut line = String::new();
    let n = stdout
        .read_line(&mut line)
        .expect("read initialize response");
    if n == 0 {
        panic!("server closed stdout before responding to initialize");
    }

    // 2. send tools/list
    let list_req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    });
    writeln!(stdin, "{list_req}").expect("write tools/list");
    stdin.flush().expect("flush tools/list");

    loop {
        line.clear();
        let n = stdout.read_line(&mut line).expect("read response");
        if n == 0 {
            panic!("server closed stdout before responding to tools/list");
        }
        if let Ok(msg) = serde_json::from_str::<Value>(&line) {
            if msg.get("id") == Some(&json!(2)) {
                return msg.get("result").cloned().unwrap_or(msg);
            }
        }
    }
}

// ----------------------------------------------------------------------------- 1. Ballot HTTP Dispatch

#[test]
fn ballot_http_dispatch_sanitizes_fences_and_preserves_elder_tools() {
    // Start a local HTTP mock server
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind listener");
    let port = listener.local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{port}/v1/chat/completions");

    let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let running_clone = running.clone();
    let request_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count_clone = request_count.clone();

    let server_handle = std::thread::spawn(move || {
        listener.set_nonblocking(true).unwrap();
        while running_clone.load(std::sync::atomic::Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let req_num = count_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    if req_num >= 10 {
                        break;
                    }
                    std::thread::spawn(move || {
                        stream
                            .set_read_timeout(Some(Duration::from_secs(5)))
                            .unwrap();
                        let mut buf = Vec::new();
                        let mut temp = [0u8; 2048];
                        let mut content_length = None;
                        let mut header_end = None;
                        while let Ok(n) = stream.read(&mut temp) {
                            if n == 0 {
                                break;
                            }
                            buf.extend_from_slice(&temp[..n]);
                            if header_end.is_none() {
                                if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                                    header_end = Some(pos + 4);
                                    let headers = String::from_utf8_lossy(&buf[..pos]);
                                    for line in headers.lines() {
                                        if let Some(val) =
                                            line.to_lowercase().strip_prefix("content-length:")
                                        {
                                            content_length = val.trim().parse::<usize>().ok();
                                        }
                                    }
                                }
                            }
                            if let (Some(h_end), Some(cl)) = (header_end, content_length) {
                                if buf.len() >= h_end + cl {
                                    break;
                                }
                            }
                        }
                        if let (Some(h_end), Some(cl)) = (header_end, content_length) {
                            let body =
                                String::from_utf8_lossy(&buf[h_end..(h_end + cl).min(buf.len())]);
                            let seat = if body.contains("seat L1") {
                                "L1"
                            } else if body.contains("seat L2") {
                                "L2"
                            } else if body.contains("seat L3") {
                                "L3"
                            } else {
                                "L1"
                            };
                            let content = format!(
                                "```json\n{{\"voter\": \"{seat}\", \"ranking\": [\"A\", \"B\", \"C\"], \"reasons\": {{\"A\": \"R1: best\", \"B\": \"R2: second\"}}, \"best_on\": {{\"R1\": \"A\", \"R2\": \"B\", \"R3\": \"C\"}}}}\n```"
                            );
                            let resp_body = json!({
                                "choices": [{
                                    "message": {
                                        "content": content
                                    }
                                }]
                            });
                            let resp_bytes = serde_json::to_vec(&resp_body).unwrap();
                            let header = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                resp_bytes.len()
                            );
                            let _ = stream.write_all(header.as_bytes());
                            let _ = stream.write_all(&resp_bytes);
                            let _ = stream.flush();
                        }
                    });
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    });

    let s = Sandbox::new("ballot-http-dispatch");

    // Write keyfile and ~/.council/config.yaml
    let keyfile_path = s.base.join("home/.council/openrouter.key");
    std::fs::create_dir_all(keyfile_path.parent().unwrap()).unwrap();
    std::fs::write(&keyfile_path, "sk-or-test-123\n").unwrap();

    let config_path = s.base.join("home/.council/config.yaml");
    let keyfile_str = keyfile_path.display().to_string().replace('\\', "/");
    std::fs::write(
        &config_path,
        format!("openrouter:\n  api_key_file: \"{keyfile_str}\"\n"),
    )
    .unwrap();

    let env_pairs = [
        ("COUNCIL_OPENROUTER_URL", url.as_str()),
        ("CLAUDECODE", "1"),
        ("PATH", ""),
    ];

    // Init Lite council run
    let init_out = s.run_with(
        &[
            "init",
            "Pick the queue technology for the billing pipeline",
            "--tier",
            "lite",
            "--mode",
            "crosslab",
            "--profile",
            "efficient",
            "--no-research",
            "--map",
            "never",
        ],
        &env_pairs,
        None,
    );
    assert_eq!(init_out.code, 0, "init failed: {}", init_out.all());

    // Find run directory
    let run_dir = {
        let runs_dir = s.proj.join(".council/runs");
        let mut entries: Vec<_> = std::fs::read_dir(&runs_dir)
            .expect("read .council/runs")
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .collect();
        entries.sort_by_key(|e| e.file_name());
        entries.last().expect("at least one run dir").path()
    };

    // Fill valid brief.yaml
    let brief_yaml = r#"restated: "Choose how billing events travel from an order to the ledger, so that no customer is charged twice."
eli5: "Pick the safest way to carry bills from the shop to the ledger."
rubric:
  - {id: R1, name: "no double charge, no lost event", weight: 40, check: judgment}
  - {id: R2, name: "cost to run", weight: 30, check: judgment}
  - {id: R3, name: "simple to operate", weight: 30, check: judgment}
evidence_plan: ["replay last month's events through the candidate", "a load test at the stated peak"]
research_questions:
  - "How do billing systems stop a retried queue delivery from charging a customer twice?"
lenses:
  - {id: L1, name: "least that can go wrong", maximize: "reliability for every customer", respect: "the budget", method: "work back from the worst failure", evidence: "the facts on file and the code"}
  - {id: L2, name: "least to build and run", maximize: "simplicity", respect: "the stated peak load", method: "delete parts until it breaks", evidence: "the facts on file and the code"}
  - {id: L3, name: "room to grow", maximize: "throughput and replay", respect: "what the team can operate", method: "size it for ten times the load", evidence: "the facts on file and the code"}
facts: "Peak is 4,000 events per second. Last month 0.4% of retried events were billed twice."
lessons_applied: []
approved_by: "requester"
"#;
    std::fs::write(run_dir.join("brief.yaml"), brief_yaml).unwrap();

    let ingest_brief = s.run_with(&["ingest"], &env_pairs, None);
    assert_eq!(ingest_brief.code, 0, "ingest brief: {}", ingest_brief.all());

    let next_brief = s.run_with(&["next"], &env_pairs, None);
    assert_eq!(next_brief.code, 0, "next brief: {}", next_brief.all());

    // At ideate: verify dispatch does NOT send subagent ideation seats over HTTP if they need read tools
    let ideate_reqs_before = request_count.load(std::sync::atomic::Ordering::SeqCst);
    let dispatch_ideate = s.run_with(&["dispatch"], &env_pairs, None);
    let ideate_reqs_after = request_count.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        ideate_reqs_before, ideate_reqs_after,
        "ideate seats needing read tools must not be dispatched over tool-less HTTP"
    );
    let _ = dispatch_ideate;

    // Write valid L1, L2, L3, control outputs
    let loop1_dir = run_dir.join("loop-1");
    council_loop::demo::script::play_seats(&loop1_dir.join("ideate")).unwrap();
    if run_dir.join("control").exists() {
        let _ = council_loop::demo::script::play_seats(&run_dir.join("control"));
    }

    let ingest_ideate = s.run_with(&["ingest"], &env_pairs, None);
    assert_eq!(
        ingest_ideate.code,
        0,
        "ingest ideate: {}",
        ingest_ideate.all()
    );

    // In merge phase: set reviewed: true in pool.json
    council_loop::demo::script::approve_pool(&loop1_dir.join("pool.json")).unwrap();

    let ingest_merge = s.run_with(&["ingest"], &env_pairs, None);
    assert_eq!(ingest_merge.code, 0, "ingest merge: {}", ingest_merge.all());

    // In redteam phase: write valid RT.out.json
    council_loop::demo::script::play_seats(&loop1_dir.join("redteam")).unwrap();

    let ingest_rt = s.run_with(&["ingest"], &env_pairs, None);
    assert_eq!(ingest_rt.code, 0, "ingest redteam: {}", ingest_rt.all());

    // In fuse phase: fill candidates.json responses
    council_loop::demo::script::fill_fuse(&loop1_dir.join("candidates.json")).unwrap();

    let ingest_fuse = s.run_with(&["ingest"], &env_pairs, None);
    assert_eq!(ingest_fuse.code, 0, "ingest fuse: {}", ingest_fuse.all());

    let next_fuse = s.run_with(&["next"], &env_pairs, None);
    assert_eq!(next_fuse.code, 0, "next fuse: {}", next_fuse.all());

    // Now at phase == "rank"!
    // Dispatch ballots over HTTP (with NO OPENROUTER_API_KEY env var)
    let dispatch_rank = s.run_with(&["dispatch"], &env_pairs, None);
    assert_eq!(
        dispatch_rank.code,
        0,
        "dispatch rank failed: {}",
        dispatch_rank.all()
    );

    let rank_dir = loop1_dir.join("rank");
    for seat in ["L1", "L2", "L3"] {
        let out_file = rank_dir.join(format!("{seat}.out.json"));
        assert!(out_file.exists(), "{seat}.out.json does not exist");
        let content = std::fs::read_to_string(&out_file).unwrap();
        assert!(
            !content.contains("```"),
            "{seat}.out.json contains markdown fences: {content}"
        );
        let parsed: Value = serde_json::from_str(&content).expect("valid JSON ballot");
        assert_eq!(parsed["voter"], seat);
        assert_eq!(parsed["ranking"], json!(["A", "B", "C"]));
    }

    // Ingest rank ballots -> advances to elders!
    let ingest_rank = s.run_with(&["ingest"], &env_pairs, None);
    assert_eq!(
        ingest_rank.code,
        0,
        "ingest rank failed: {}",
        ingest_rank.all()
    );

    let run_state: Value =
        serde_json::from_str(&std::fs::read_to_string(run_dir.join("run.json")).unwrap()).unwrap();
    assert_eq!(run_state["phase"], "elders");

    // At elders: run dispatch when EA has via == "subagent"
    assert_eq!(
        run_state["seats"]["EA"]["via"], "subagent",
        "EA seat should have via == subagent"
    );
    let dispatch_elders = s.run_with(&["dispatch"], &env_pairs, None);
    let _ = dispatch_elders;

    // Assert EA.out.json does NOT exist yet (Elder seats are NEVER stripped of verification tools)
    let ea_out = loop1_dir.join("elders/EA.out.json");
    assert!(
        !ea_out.exists(),
        "EA.out.json must NOT exist yet (Elder seats must not be auto-diverted to tool-less HTTP)"
    );

    // Clean up mock server
    running.store(false, std::sync::atomic::Ordering::SeqCst);
    let _ = server_handle.join();
}

// ----------------------------------------------------------------------------- 2. Compact MCP Schemas

#[test]
fn compact_mcp_schemas_shrink_wire_size_and_keep_all_tools() {
    let s = Sandbox::new("mcp-compact");

    let full_result = mcp_list_tools(&s, &[]);
    let full_tools = full_result["tools"]
        .as_array()
        .expect("tools array in full result");
    assert_eq!(full_tools.len(), 27, "full tools must have 27 tools");

    let compact_result = mcp_list_tools(&s, &["--compact-schemas"]);
    let compact_tools = compact_result["tools"]
        .as_array()
        .expect("tools array in compact result");
    assert_eq!(compact_tools.len(), 27, "compact tools must have 27 tools");

    let required_tools = [
        "council_init",
        "council_dispatch",
        "council_deviation",
        "council_audit",
        "council_elder_review",
    ];
    for name in required_tools {
        assert!(
            full_tools.iter().any(|t| t["name"] == name),
            "full tools missing {name}"
        );
        assert!(
            compact_tools.iter().any(|t| t["name"] == name),
            "compact tools missing {name}"
        );
    }

    for comp in compact_tools {
        let name = comp["name"].as_str().unwrap();
        let full = full_tools
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("tool {name} not found in full"));

        let comp_props = comp["inputSchema"]["properties"]
            .as_object()
            .map(|m| m.keys().cloned().collect::<std::collections::BTreeSet<_>>())
            .unwrap_or_default();
        let full_props = full["inputSchema"]["properties"]
            .as_object()
            .map(|m| m.keys().cloned().collect::<std::collections::BTreeSet<_>>())
            .unwrap_or_default();
        assert_eq!(comp_props, full_props, "property keys mismatch for {name}");

        let comp_req = comp["inputSchema"]["required"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect::<std::collections::BTreeSet<_>>()
            })
            .unwrap_or_default();
        let full_req = full["inputSchema"]["required"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect::<std::collections::BTreeSet<_>>()
            })
            .unwrap_or_default();
        assert_eq!(comp_req, full_req, "required mismatch for {name}");
    }

    let full_bytes = serde_json::to_vec(full_tools).unwrap();
    let compact_bytes = serde_json::to_vec(compact_tools).unwrap();
    assert!(
        full_bytes.len() > 20_000,
        "expected full tools serialized bytes > 20,000, got {}",
        full_bytes.len()
    );
    assert!(
        compact_bytes.len() <= 10_500,
        "expected compact tools serialized bytes <= 10,500, got {}",
        compact_bytes.len()
    );
}

// ----------------------------------------------------------------------------- 3. Doctor Probe 401

#[test]
fn doctor_probe_reports_expired_openrouter_key_401() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind listener");
    let port = listener.local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{port}/v1/chat/completions");

    let server_handle = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let resp = "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: 43\r\nConnection: close\r\n\r\n{\"error\":{\"message\":\"API key expired\"}}";
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.flush();
        }
    });

    let s = Sandbox::new("doctor-probe-401");
    let out = s.run_with(
        &["doctor", "--probe"],
        &[
            ("OPENROUTER_API_KEY", "sk-expired"),
            ("COUNCIL_OPENROUTER_URL", &url),
            ("PATH", ""),
        ],
        None,
    );
    assert_eq!(out.code, 0, "doctor --probe failed: {}", out.all());

    let first = out.out.find('{').expect("found opening { in doctor stdout");
    let last = out
        .out
        .rfind('}')
        .expect("found closing } in doctor stdout");
    let json_str = &out.out[first..=last];
    let val: Value = serde_json::from_str(json_str).expect("valid JSON in doctor stdout");

    let probe = val["probes"]["openrouter"]
        .as_str()
        .expect("probes.openrouter must be a string");
    assert!(
        probe.starts_with("FAILED"),
        "expected probes.openrouter to start with FAILED, got: {probe}"
    );

    let notes_text = val["notes"].to_string().to_lowercase();
    assert!(
        notes_text.contains("openrouter")
            || notes_text.contains("401")
            || notes_text.contains("expired"),
        "expected notes to mention openrouter/401/expired, got: {notes_text}"
    );

    let _ = server_handle.join();
}

// ----------------------------------------------------------------------------- 4. Antigravity Hook

#[test]
fn antigravity_camelcase_hook_denies_locked_edits_and_case_mismatched_paths() {
    let s = Sandbox::new("hook-antigravity");
    s.write("tests/test_locked.py", "def test_locked(): assert True\n");
    let lock_out = s.run(&["lock-tests", "tests"]);
    assert_eq!(lock_out.code, 0, "lock-tests failed: {}", lock_out.all());

    // Antigravity camelCase PreToolUse payload for edit_file
    let agy_edit = json!({
        "hookEventName": "PreToolUse",
        "cwd": s.proj.to_string_lossy(),
        "toolCall": {
            "id": "1",
            "name": "edit_file",
            "args": {
                "Path": "tests/test_locked.py"
            }
        }
    });
    let raw_edit = serde_json::to_vec(&agy_edit).unwrap();
    let res1 = council_loop::hook::decide(&raw_edit);
    assert!(
        res1.is_some(),
        "expected decide to return Some for antigravity edit_file"
    );
    let parsed1: Value = serde_json::from_str(&res1.unwrap()).unwrap();
    assert_eq!(parsed1["decision"], "deny");
    assert!(
        parsed1["reason"].as_str().is_some_and(|r| !r.is_empty()),
        "reason must be non-empty"
    );
    assert!(
        parsed1.get("hookSpecificOutput").is_none(),
        "antigravity decision must NOT use hookSpecificOutput"
    );

    // Antigravity run_command payload writing to locked file
    let agy_cmd = json!({
        "hookEventName": "PreToolUse",
        "cwd": s.proj.to_string_lossy(),
        "toolCall": {
            "id": "2",
            "name": "run_command",
            "args": {
                "CommandLine": "echo x > tests/test_locked.py"
            }
        }
    });
    let raw_cmd = serde_json::to_vec(&agy_cmd).unwrap();
    let res2 = council_loop::hook::decide(&raw_cmd);
    assert!(
        res2.is_some(),
        "expected decide to return Some for antigravity run_command writing to locked file"
    );
    let parsed2: Value = serde_json::from_str(&res2.unwrap()).unwrap();
    assert_eq!(parsed2["decision"], "deny");
    assert!(
        parsed2["reason"].as_str().is_some_and(|r| !r.is_empty()),
        "reason must be non-empty"
    );

    // On Windows: uppercase path in both Claude (Edit) and Antigravity (write_file)
    #[cfg(windows)]
    {
        let claude_upper = json!({
            "hook_event_name": "PreToolUse",
            "cwd": s.proj.to_string_lossy(),
            "tool_name": "Edit",
            "tool_input": {
                "file_path": "TESTS/TEST_LOCKED.PY"
            }
        });
        let res_claude_upper =
            council_loop::hook::decide(&serde_json::to_vec(&claude_upper).unwrap());
        assert!(
            res_claude_upper.is_some(),
            "expected decide to deny uppercase Claude Edit on Windows"
        );
        let parsed_cu: Value = serde_json::from_str(&res_claude_upper.unwrap()).unwrap();
        assert_eq!(
            parsed_cu["hookSpecificOutput"]["permissionDecision"],
            "deny"
        );

        let agy_upper = json!({
            "hookEventName": "PreToolUse",
            "cwd": s.proj.to_string_lossy(),
            "toolCall": {
                "id": "3",
                "name": "write_file",
                "args": {
                    "Path": "TESTS/TEST_LOCKED.PY"
                }
            }
        });
        let res_agy_upper = council_loop::hook::decide(&serde_json::to_vec(&agy_upper).unwrap());
        assert!(
            res_agy_upper.is_some(),
            "expected decide to deny uppercase Antigravity write_file on Windows"
        );
        let parsed_au: Value = serde_json::from_str(&res_agy_upper.unwrap()).unwrap();
        assert_eq!(parsed_au["decision"], "deny");
    }

    // Verify normal Claude Code Edit payload still returns hookSpecificOutput.permissionDecision == "deny"
    let claude_edit = json!({
        "hook_event_name": "PreToolUse",
        "cwd": s.proj.to_string_lossy(),
        "tool_name": "Edit",
        "tool_input": {
            "file_path": "tests/test_locked.py"
        }
    });
    let res_claude = council_loop::hook::decide(&serde_json::to_vec(&claude_edit).unwrap());
    assert!(
        res_claude.is_some(),
        "expected decide to deny normal Claude Edit"
    );
    let parsed_ce: Value = serde_json::from_str(&res_claude.unwrap()).unwrap();
    assert_eq!(
        parsed_ce["hookSpecificOutput"]["permissionDecision"],
        "deny"
    );
}

// ----------------------------------------------------------------------------- 5. Memory Clean Fact

#[test]
fn windows_drive_letter_sources_and_line_anchors_pass_memory_clean_fact() {
    let s = Sandbox::new("memory-clean-fact");
    s.write("src/example.rs", "pub fn answer() -> i32 { 42 }\n");

    // Relative source with line anchor
    let fact1 = json!({
        "claim": "The billing queue service guarantees idempotent processing of each message",
        "source": "src/example.rs:15"
    });
    let res1 = council_loop::memory::clean_fact(&fact1, &s.proj);
    assert!(
        res1.is_ok(),
        "clean_fact failed on relative source with line anchor: {:?}",
        res1.err()
    );

    // Full absolute path
    let full_path = s.proj.join("src/example.rs");
    let fact2 = json!({
        "claim": "The billing queue service guarantees idempotent processing of each message",
        "source": full_path.to_string_lossy()
    });
    let res2 = council_loop::memory::clean_fact(&fact2, &s.proj);
    assert!(
        res2.is_ok(),
        "clean_fact failed on full absolute path: {:?}",
        res2.err()
    );

    // On Windows: lowercase drive letter `c:/...` and `:20` suffix
    #[cfg(windows)]
    {
        let path_str = full_path.to_string_lossy().replace('\\', "/");
        let path_str = path_str.strip_prefix("//?/").unwrap_or(&path_str);
        let lower_drive = if path_str.len() >= 2 && path_str.as_bytes()[1] == b':' {
            format!("{}:{}", path_str[..1].to_lowercase(), &path_str[2..])
        } else {
            path_str.to_string()
        };
        let src_with_anchor = format!("{lower_drive}:20");
        let fact3 = json!({
            "claim": "The billing queue service guarantees idempotent processing of each message",
            "source": src_with_anchor
        });
        let res3 = council_loop::memory::clean_fact(&fact3, &s.proj);
        assert!(
            res3.is_ok(),
            "clean_fact failed on Windows drive letter with anchor: {:?}",
            res3.err()
        );
    }
}
