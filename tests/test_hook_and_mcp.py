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
    p = subprocess.run([sys.executable, str(HOOK)], input=json.dumps(event), capture_output=True, text=True,
                       encoding="utf-8", env=env)
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


def test_mcp_server_lists_tools_resources_prompts():
    pytest.importorskip("mcp.client.stdio")
    import asyncio
    from mcp import ClientSession, StdioServerParameters
    from mcp.client.stdio import stdio_client

    async def go():
        params = StdioServerParameters(command=sys.executable, args=[str(ROOT / "mcp" / "server.py")])
        async with stdio_client(params) as (r, w):
            async with ClientSession(r, w) as s:
                await s.initialize()
                tools = {t.name for t in (await s.list_tools()).tools}
                res = {str(x.uri) for x in (await s.list_resources()).resources}
                prompts = {p.name for p in (await s.list_prompts()).prompts}
                skill = await s.read_resource("skill://council-loop/SKILL.md")
                return tools, res, prompts, skill

    tools, res, prompts, skill = asyncio.run(go())
    assert {"council_plan", "council_models", "council_blind", "council_tally", "council_check_run",
            "council_verify_locks", "council_elder_review"} <= tools
    assert "skill://council-loop/SKILL.md" in res and "council_loop" in prompts
    assert "name: council-loop" in skill.contents[0].text
