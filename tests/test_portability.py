"""The kit must not depend on the machine's locale, its path separator, its line ends or its shell.

What is tested is the class of failure Windows brings: a default text encoding that is not UTF-8
(simulated here with an ASCII locale, which is stricter than a Windows code page), backslashes in
paths, CRLF line ends, byte-order marks and tools that live behind .cmd files. The whole suite also
passes as a Windows program under Wine (docs/PORTING.md); it has not run on a real Windows machine.
"""
import ast
import json
import os
import shutil
import subprocess
import sys
import unicodedata
from pathlib import Path

import pytest

import codemap
import council
import memory
import providers

ROOT = Path(__file__).resolve().parents[1]
SCRIPTS = ROOT / "skills" / "council-loop" / "scripts"
CLI = SCRIPTS / "council.py"
HOOK = ROOT / "hooks" / "protect_locked_files.py"
SHIPPED = [*sorted(SCRIPTS.glob("*.py")), ROOT / "mcp" / "server.py", *sorted((ROOT / "hooks").glob("*.py"))]
CODE = [*SHIPPED, *sorted((ROOT / "tests").glob("*.py"))]


utf8_names = pytest.mark.skipif(sys.getfilesystemencoding().lower().replace("-", "") != "utf8",
                                reason="this test itself passes arguments and file names that are not ASCII")


def ascii_env(**extra) -> dict:
    """An interpreter whose default text encoding is ASCII: no arrow, no accent, nothing above 127."""
    env = {k: v for k, v in os.environ.items() if k not in ("PYTHONIOENCODING", "LC_CTYPE", "LANGUAGE", "CLAUDECODE")}
    env.update(LC_ALL="C", LANG="C", PYTHONUTF8="0", PYTHONCOERCECLOCALE="0", **extra)
    return env


def cli(args, cwd, **env):
    p = subprocess.run([sys.executable, str(CLI), *args], cwd=cwd, env=ascii_env(**env), capture_output=True)
    out, err = p.stdout.decode("utf-8"), p.stderr.decode("utf-8")       # strict: whatever is printed is UTF-8
    assert "Traceback" not in err and "codec can't" not in err, err
    return p.returncode, out, err


def unsafe_calls(path: Path):
    """Calls that would read or write text in the machine's own encoding."""
    for node in ast.walk(ast.parse(path.read_text(encoding="utf-8"))):
        if not isinstance(node, ast.Call):
            continue
        f, kw = node.func, {k.arg for k in node.keywords}
        attr = f.attr if isinstance(f, ast.Attribute) else None
        base = getattr(getattr(f, "value", None), "id", "")
        if attr == "read_text" and not node.args and "encoding" not in kw:
            yield node.lineno, "read_text() without an encoding"
        elif attr == "write_text" and len(node.args) == 1 and "encoding" not in kw:
            yield node.lineno, "write_text() without an encoding"
        elif (isinstance(f, ast.Name) and f.id == "open") or (attr == "open" and base not in ("os", "webbrowser", "tarfile", "zipfile")):
            at = 1 if isinstance(f, ast.Name) else 0
            mode = node.args[at] if len(node.args) > at else next((k.value for k in node.keywords if k.arg == "mode"), None)
            if not (isinstance(mode, ast.Constant) and "b" in str(mode.value)) and "encoding" not in kw:
                yield node.lineno, "open() in text mode without an encoding"
        elif base == "subprocess" and attr in ("run", "Popen", "check_output", "check_call", "call"):
            texty = any(k.arg in ("text", "universal_newlines") and getattr(k.value, "value", False) for k in node.keywords)
            if texty and "encoding" not in kw:
                yield node.lineno, f"subprocess.{attr}(text=True) without an encoding"


def test_every_text_read_write_and_pipe_names_its_encoding():
    found = [f"{p.relative_to(ROOT).as_posix()}:{line}: {what}" for p in CODE for line, what in unsafe_calls(p)]
    assert not found, "these depend on the machine's locale (cp1252 on Windows):\n" + "\n".join(found)


def test_every_file_and_every_line_printed_ends_lines_the_same_way_on_every_system():
    """Text mode on Windows turns \\n into \\r\\n, in files and on a pipe. The Rust binary never does, so the
    reference must not either: the two write the same bytes on every system. Checked in the source,
    because the effect only shows on Windows."""
    found = []
    for p in SHIPPED:
        for node in ast.walk(ast.parse(p.read_text(encoding="utf-8"))):
            if not (isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute)):
                continue
            newline = next((getattr(k.value, "value", None) for k in node.keywords if k.arg == "newline"), None)
            if node.func.attr in ("write_text", "reconfigure") and newline != "\n":
                found.append(f'{p.relative_to(ROOT).as_posix()}:{node.lineno}: {node.func.attr}() without newline="\\n"')
    assert not found, "these write \\r\\n on Windows:\n" + "\n".join(found)
    for script in (CLI, SCRIPTS / "refresh_models.py", HOOK):       # every entry point sets up its console
        assert "reconfigure(" in script.read_text(encoding="utf-8") or "utf8_console()" in script.read_text(encoding="utf-8"), script


@utf8_names
def test_the_commands_work_when_the_locale_is_not_utf8(tmp_path):
    (tmp_path / "src").mkdir()
    (tmp_path / "src" / "ledger.py").write_text("def post(event):\n    return event  # naïve → fine\n", encoding="utf-8")
    home = str(tmp_path / "home")
    ask = "Pick the queue for the “billing” pipeline — naïve or safe → which?"
    rc, out, _ = cli(["plan", ask, "--stakes", "high", "--reversible", "no"], tmp_path, COUNCIL_HOME=home)
    assert rc == 0 and "GATE tier=full" in out and "MEMBERS researchers=2" in out
    rc, out, _ = cli(["init", ask, "--tier", "lite", "--research", "1", "--paths", "src/ledger.py"], tmp_path, COUNCIL_HOME=home)
    assert rc == 0 and "GATE tier=lite" in out
    run = json.loads(next((tmp_path / ".council" / "runs").glob("*/run.json")).read_text(encoding="utf-8"))
    assert run["ask"] == ask, "the ask is kept letter for letter, whatever the locale"
    for args, expect in ((["next"], "brief.yaml"), (["status"], "brief"), (["config"], "members.research.seats"),
                         (["config", "--example"], "members:"), (["doctor"], "mode"), (["map"], "skeleton"),
                         (["map", "--focus", "post the ledger"], ""), (["memory", "status"], "research notes")):
        rc, out, err = cli(args, tmp_path, COUNCIL_HOME=home)
        assert rc == 0 and expect in out, (args, rc, out[-300:], err[-300:])
    rc, out, _ = cli(["memory", "add", "--claim", "Queue A keeps order per partition only — not across",
                      "--source", "https://example.org/queue-a/ordering", "--quote", "“order is guaranteed per partition”"],
                     tmp_path, COUNCIL_HOME=home)
    assert rc == 0 and "added" in out
    rc, out, _ = cli(["memory", "search", "order partition"], tmp_path, COUNCIL_HOME=home)
    assert rc == 0 and "per partition only — not across" in out


@pytest.mark.parametrize("which", ["tests/test_run.py::test_lite_run_end_to_end",
                                   "tests/test_run.py::test_cli_seats_dispatch_and_a_rejected_one_can_really_be_retried",
                                   "tests/test_members.py", "tests/test_memory.py", "tests/test_codemap.py"])
def test_the_run_loop_itself_passes_under_a_locale_that_is_not_utf8(which):
    if os.environ.get("COUNCIL_NESTED") == "1":
        pytest.skip("already inside the nested run")
    p = subprocess.run([sys.executable, "-m", "pytest", "-q", "-x", "-p", "no:cacheprovider", which], cwd=ROOT,
                       env=ascii_env(COUNCIL_NESTED="1"), capture_output=True)
    assert p.returncode == 0, p.stdout.decode("utf-8", "replace")[-3000:] + p.stderr.decode("utf-8", "replace")[-1500:]


def test_files_made_on_windows_still_load(proj):
    text = "members:\n  research:\n    seats: 1\n    budget: {lite: 7}\n"
    conf = proj / ".council" / "config.yaml"
    for raw in (b"\xff\xfe" + text.replace("\n", "\r\n").encode("utf-16-le"),      # PowerShell 5: `council.py config --example > file`
                b"\xef\xbb\xbf" + text.replace("\n", "\r\n").encode("utf-8"),       # an editor's byte-order mark, CRLF line ends
                text.encode("utf-8")):
        conf.write_bytes(raw)
        assert council.read_text(conf) == text
        got = council.members(proj)
        assert got["research"]["seats"] == 1 and got["research"]["budget"]["lite"] == 7 and not got["problems"]
    odd = proj / "odd.json"
    odd.write_bytes(b'{"claim": "caf\xe9"}')                                        # a stray Latin-1 byte: replaced, never fatal
    assert json.loads(council.read_text(odd))["claim"].startswith("caf")


def test_a_lock_taken_with_crlf_line_ends_matches_the_same_file_with_lf(proj):
    (proj / "tests").mkdir()
    f = proj / "tests" / "test_x.py"
    f.write_bytes(b"def test_x():\r\n    assert True\r\n")
    assert council.main(["lock-tests", "tests"]) == 0
    assert all("\\" not in name for name in json.loads((proj / ".council" / "locks.json").read_text(encoding="utf-8")))
    f.write_bytes(b"def test_x():\n    assert True\n")                              # the same file after git normalised it
    assert council.main(["verify-locks"]) == 0
    f.write_bytes(b"def test_x():\n    assert 1\n")
    assert council.main(["verify-locks"]) == 1


def test_the_hook_knows_a_locked_file_whichever_slash_names_it(tmp_path):
    (tmp_path / ".council").mkdir()
    (tmp_path / ".council" / "locks.json").write_bytes(b"\xef\xbb\xbf" + json.dumps({"tests/test_x.py": "abc"}).encode())

    def hook(event):
        env = {**ascii_env(), "CLAUDE_PROJECT_DIR": str(tmp_path)}
        env.pop("COUNCIL_ALLOW_LOCKED", None)
        p = subprocess.run([sys.executable, str(HOOK)], input=json.dumps(event, ensure_ascii=False).encode("utf-8"),
                           capture_output=True, env=env, cwd=tmp_path)
        assert p.returncode == 0 and b"Traceback" not in p.stderr, p.stderr
        return json.loads(p.stdout) if p.stdout.strip() else None

    for path in ("tests\\test_x.py", "tests/test_x.py", str(tmp_path / "tests" / "test_x.py")):
        assert hook({"tool_name": "Edit", "tool_input": {"file_path": path}})["hookSpecificOutput"]["permissionDecision"] == "deny"
    for command in ("del tests\\test_x.py", "Remove-Item tests\\test_x.py", 'Set-Content tests\\test_x.py "x"'):
        assert hook({"tool_name": "Bash", "tool_input": {"command": command}}), command
    assert hook({"tool_name": "Edit", "tool_input": {"file_path": "src\\café.py"}}) is None
    assert hook({"tool_name": "Bash", "tool_input": {"command": "type tests\\test_x.py"}}) is None


def test_a_tool_behind_a_cmd_file_is_run_by_its_full_path_and_read_as_utf8(monkeypatch):
    seen = {}

    def fake_run(cmd, **kw):
        seen.update(cmd=cmd, kw=kw)
        return subprocess.CompletedProcess(cmd, 0, stdout='{"ok": "naïve → fine"}\r\nsecond line\r\n'.encode("utf-8"), stderr=b"")

    monkeypatch.setattr(providers.shutil, "which", lambda name: rf"C:\Users\me\AppData\Roaming\npm\{name}.cmd")
    monkeypatch.setattr(providers.subprocess, "run", fake_run)
    res = providers.ask("openai", "Read and follow the packet\nline two → three", timeout=5)
    assert seen["cmd"][0].endswith("codex.cmd"), "npm installs codex and gemini as .cmd files on Windows"
    # bytes both ways: a text pipe would write \r\n for every \n of the prompt on Windows and read the reply as cp1252
    assert seen["kw"]["input"] == "Read and follow the packet\nline two → three".encode("utf-8")
    assert not {"text", "encoding", "universal_newlines"} & set(seen["kw"])
    assert res["ok"] and "→" in res["text"] and "\r" not in res["text"]


@utf8_names
def test_the_code_scan_reads_file_names_git_would_quote(proj):
    if not shutil.which("git"):
        pytest.skip("git is not installed")
    (proj / "shop").mkdir()
    (proj / "shop" / "café.py").write_text("def total():\n    return 1\n", encoding="utf-8")
    (proj / "shop" / "two words.py").write_text("def other():\n    return 2\n", encoding="utf-8")
    if subprocess.run(["git", "init", "-q", str(proj)], capture_output=True).returncode != 0:
        pytest.skip("git could not start a repository here")
    names = {unicodedata.normalize("NFC", n) for n in codemap.scan(proj)["files"]}
    assert {"shop/café.py", "shop/two words.py"} <= names


def test_crlf_and_lf_checkouts_of_the_same_code_are_the_same_structure(proj):
    (proj / "web").mkdir()
    js = "export function total(items) {\n  return items.length;\n}\n\nexport class Cart {\n  add(item) {}\n}\n"
    py = "import os\n\n\ndef bill(order, retries: int = 2):\n    return os.getpid()\n"
    go = "package shop\n\nfunc Total(items []int) int {\n\treturn len(items)\n}\n"
    prints = []
    for convert in (lambda t: t.encode("utf-8"), lambda t: t.replace("\n", "\r\n").encode("utf-8"),
                    lambda t: b"\xef\xbb\xbf" + t.replace("\n", "\r\n").encode("utf-8")):
        (proj / "web" / "cart.js").write_bytes(convert(js))
        (proj / "web" / "bill.py").write_bytes(convert(py))
        (proj / "web" / "total.go").write_bytes(convert(go))
        scanned = codemap.scan(proj)
        assert all(not s["name"].endswith("\r") and "\r" not in str(s.get("sig") or "")
                   for info in scanned["files"].values() for s in info["symbols"])
        prints.append(codemap.structure(scanned))
    assert prints[0] == prints[1] == prints[2] and len(prints[0]) == 3, "two machines must agree on whether a map is stale"


def test_a_note_is_never_given_a_name_windows_cannot_create():
    assert memory.slugify("CON") == "note-con" and memory.slugify("aux") == "note-aux" and memory.slugify("Com1") == "note-com1"
    assert memory.slugify("Console output is buffered") == "console-output-buffered"
