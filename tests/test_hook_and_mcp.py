import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
HOOK = ROOT / "hooks" / "protect_locked_files.py"


def run_hook(tmp_path, event):
    env = {**os.environ, "CLAUDE_PROJECT_DIR": str(tmp_path)}
    env.pop("COUNCIL_ALLOW_LOCKED", None)
    p = subprocess.run([sys.executable, str(HOOK)], input=json.dumps(event), capture_output=True, encoding="utf-8", env=env)
    return json.loads(p.stdout) if p.stdout.strip() else None


def test_hook_denies_edit_of_locked_file(tmp_path):
    (tmp_path / ".council").mkdir()
    (tmp_path / ".council" / "locks.json").write_text(json.dumps({"tests/test_x.py": "abc"}), encoding="utf-8")
    out = run_hook(tmp_path, {"tool_name": "Edit", "tool_input": {"file_path": str(tmp_path / "tests/test_x.py")}})
    assert out["hookSpecificOutput"]["permissionDecision"] == "deny"
    assert run_hook(tmp_path, {"tool_name": "Edit", "tool_input": {"file_path": str(tmp_path / "src/app.py")}}) is None
    out = run_hook(tmp_path, {"tool_name": "Bash", "tool_input": {"command": "sed -i 's/1/2/' tests/test_x.py"}})
    assert out["hookSpecificOutput"]["permissionDecision"] == "deny"
    assert run_hook(tmp_path, {"tool_name": "Bash", "tool_input": {"command": "cat tests/test_x.py"}}) is None


def test_mcp_server_serves_the_skill_and_runs_the_loop(tmp_path):
    pytest.importorskip("mcp")
    import asyncio
    from mcp import ClientSession, StdioServerParameters
    from mcp.client.stdio import stdio_client

    async def go():
        env = {**os.environ, "CLAUDECODE": "1", "COUNCIL_PRIVATE_DIR": str(tmp_path / "private"),
               "COUNCIL_HOME": str(tmp_path / "home")}
        params = StdioServerParameters(command=sys.executable, args=[str(ROOT / "mcp" / "server.py")], env=env)
        async with stdio_client(params) as (r, w):
            async with ClientSession(r, w) as s:
                await s.initialize()
                tools = {t.name for t in (await s.list_tools()).tools}
                res = {str(x.uri) for x in (await s.list_resources()).resources}
                prompts = {p.name for p in (await s.list_prompts()).prompts}
                skill = await s.read_resource("skill://council-loop/SKILL.md")
                packet = await s.read_resource("council://templates/ideate")

                async def call(name, **kw):
                    out = await s.call_tool(name, kw)
                    return json.loads(out.content[0].text)

                steps = [await call("council_init", ask="Pick a queue for billing events", project_dir=str(tmp_path),
                                    tier="lite", mode="fanout:claude", research=3, mapper="never"),
                         await call("council_ingest", project_dir=str(tmp_path)),          # the brief is not written yet
                         await call("council_status", project_dir=str(tmp_path)),
                         await call("council_next", project_dir="relative/path")]
                extra = [await call("council_memory_add", project_dir=str(tmp_path), source="https://example.org/a",
                                    claim="Idempotency keys stop double charges when a queue retries"),
                         await call("council_memory_add", project_dir=str(tmp_path), source="", claim="A claim with no source at all"),
                         await call("council_memory_search", project_dir=str(tmp_path), query="idempotency keys"),
                         await call("council_config", project_dir=str(tmp_path)),
                         await call("council_map", project_dir=str(tmp_path), action="check"),
                         await call("council_plan", task="Pick a queue for billing events", tier="lite", mode="fanout:claude",
                                    project_dir=str(tmp_path), research=1)]
                return tools, res, prompts, skill, packet, steps, extra

    tools, res, prompts, skill, packet, steps, extra = asyncio.run(go())
    assert {"council_plan", "council_models", "council_blind", "council_tally", "council_check_run",
            "council_verify_locks", "council_elder_review", "council_doctor", "council_init", "council_next",
            "council_ingest", "council_dispatch", "council_cut", "council_status", "council_deviation",
            "council_audit", "council_note", "council_memory_search", "council_memory_add", "council_memory_status",
            "council_memory_setup", "council_map", "council_config"} <= tools
    assert "skill://council-loop/SKILL.md" in res and "council_loop" in prompts
    assert "name: council-loop" in skill.contents[0].text and "IDEATE" in packet.contents[0].text
    init, ingest, status, bad = steps
    assert init["ok"] and init["output"].startswith("GATE tier=lite") and "brief.yaml" in init["output"]
    assert not ingest["ok"] and "brief.yaml is not ready" in ingest["output"]              # an error is an answer, not a crash
    assert status["ok"] and "phase brief" in status["output"]
    assert not bad["ok"] and "absolute" in bad["output"]
    assert (tmp_path / ".council" / "runs" / "CURRENT").exists()
    assert "MEMBERS researchers=3 (papers and primary research; practice in the field; the case against" in init["output"]
    added, refused, found, config, map_check, plan = extra
    assert added["ok"] and "added idempotency-keys-stop" in added["output"]
    assert not refused["ok"] and "no source" in refused["output"]                          # a fact without a source is refused
    assert found["ok"] and "Idempotency keys stop double charges" in found["output"]
    assert "members.research.seats = 2    (default)" in config["output"]
    assert map_check["ok"] and "no source code found" in map_check["output"]
    assert plan["calls_estimate"] == 10 and plan["support"]["research"]["seats"] == 1
