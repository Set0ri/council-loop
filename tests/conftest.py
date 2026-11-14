import os
import shutil
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
TESTS = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT / "skills" / "council-loop" / "scripts"))


@pytest.fixture()
def proj(tmp_path, monkeypatch):
    """An empty project in a temp folder, cut off from this machine's own settings."""
    monkeypatch.chdir(tmp_path)
    monkeypatch.setenv("CLAUDECODE", "1")                    # seats run as subagents (file outputs)
    monkeypatch.setenv("COUNCIL_PRIVATE_DIR", str(tmp_path / "private"))
    monkeypatch.setenv("COUNCIL_HOME", str(tmp_path / "home"))       # the user's ~/.council, never the real one
    for name in ("COUNCIL_SEATS_VIA", "COUNCIL_MEMORY_DIR", "COUNCIL_OBSIDIAN_VAULT", "COUNCIL_OBSIDIAN_FOLDER"):
        monkeypatch.delenv(name, raising=False)
    (tmp_path / ".council").mkdir()
    (tmp_path / "docs").mkdir()
    (tmp_path / "docs" / "lessons.md").write_text(
        "- [L-20261113-1] (helpful: 3 | harmful: 0) Size the council to the stakes. Evidence: x\n", encoding="utf-8")
    return tmp_path


@pytest.fixture()
def fake_clis(proj, monkeypatch, tmp_path_factory):
    """claude, codex and gemini on PATH are tests/fakecli.py: our adapters run for real, the models do not."""
    tools = tmp_path_factory.mktemp("tools")                 # outside the project: on Windows the fakes are .py files,
    bin_dir = tools / "bin"                                  # and inside it they would count as its source code
    bin_dir.mkdir()
    for name in ("claude", "codex", "gemini"):
        if os.name == "nt":                                  # Windows runs no shebangs: a .cmd file starts the script
            shutil.copy(TESTS / "fakecli.py", bin_dir / f"{name}.py")
            (bin_dir / f"{name}.cmd").write_text(f'@"{sys.executable}" "%~dp0{name}.py" %*\r\n', encoding="utf-8")
        else:
            shutil.copy(TESTS / "fakecli.py", bin_dir / name)
            (bin_dir / name).chmod(0o755)
    (tools / "state").mkdir()
    monkeypatch.setenv("PATH", f"{bin_dir}{os.pathsep}{os.environ['PATH']}")
    monkeypatch.setenv("FAKE_CLI_TESTS", str(TESTS))
    monkeypatch.setenv("FAKE_CLI_STATE", str(tools / "state"))
    monkeypatch.delenv("CLAUDECODE", raising=False)            # no subagents: every seat goes through a CLI
    return bin_dir
