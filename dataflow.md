# Council Loop v0.4.0 — Architecture & Dataflow

This document summarizes the runtime dataflow, dual-implementation surfaces, persisted state files, and Antigravity integration contracts for `council-loop` (`v0.4.0`). For the full developer architecture map, see [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

## 1. Entry Surfaces & Parity Contract

All surfaces route through the same rules in Rust (`src/`, binary `council`) and the Python reference (`skills/council-loop/scripts/`, `mcp/server.py`, `hooks/protect_locked_files.py`), held to identical output and disk writes by `tests/test_parity.py` and `tests/test_surfaces.py`:

| Surface | Entrypoint | Role |
| --- | --- | --- |
| **Rust Binary** | `src/main.rs`, `src/cli.rs` -> `council` (`council.exe`) | Standalone binary embedding `skills/council-loop/` assets via `build.rs` and `src/assets.rs` (disk skill folder overrides compiled-in copy). |
| **Launcher** | `bin/council`, `bin/council.cmd`, `skills/council-loop/scripts/council` | Resolves `COUNCIL_BIN`, local `target/release/council`, or `PATH`, falling back to Python scripts (`council.py`, `mcp/server.py`, `hooks/protect_locked_files.py`). |
| **MCP Server** | `council mcp` (`src/mcp.rs`) & `mcp/server.py` | Stdio JSON-RPC 2.0 server (`2024-11-05`..`2025-11-25`) exposing 23 tools (`project_dir`-scoped for run-loop verbs), 2 resources (`skill://council-loop/SKILL.md`, `council://models/lock`), 2 resource templates (`council://reference/{name}`, `council://templates/{name}`), and prompt `council_loop`. |
| **Test-Lock Hook** | `council hook` (`src/hook.rs`) & `hooks/protect_locked_files.py` (via `hooks/hooks.json`) | `PreToolUse` guard blocking edits or shell writes to paths locked in `.council/locks.json` (plus `.council/locks.json` itself). |
| **Antigravity Config** | `.agents/skills.json` + `~/.gemini/config/mcp_config.json` (`council-loop`) | Workspace skill discovery (`skills/`) and global stdio `council mcp` server registration for Antigravity sessions. |
| **CI Guard** | `.github/workflows/ci.yml`, `.github/workflows/release.yml` | Runs `cargo fmt`, `clippy`, `cargo test`, `cargo build --release`, `pytest -q`, `council map --check`, `council refresh --check`, MSRV `1.85` build, and PR `verify-locks`. |

---

## 2. End-to-End Run Loop & Support-Member Dataflow

```mermaid
flowchart LR
  Ask["User Ask + Flags"] --> Init["council init / council_init<br/>src/gate.rs + src/core.rs"]
  Init -->|"Tier, seats, members"| Brief["Chair writes brief.yaml<br/>ingest freezes rubric"]
  Brief --> Support["Parallel Research & Code Map<br/>src/memory.rs + src/codemap.rs + src/pyread.rs"]
  Support -->|"harvest -> notes & ARCHITECTURE.md"| LoopStart["next -> Packet Render (src/run/prompts.rs)<br/>collect / ingest -> Validate (src/run/validate.rs)"]
  LoopStart -->|"Ideate -> Merge -> Blind Vote -> Red Team -> Fuse -> Rank -> Elders"| Cut["council cut (src/run/cut.rs)<br/>Borda + Check Priority + Wildcard"]
  Cut -->|"Next loop (<=3)"| LoopStart
  Cut -->|"Winner"| Final["Chair writes final.md + retro.md<br/>Unblind mapping & release commons"]
  Final --> Locks["council lock-tests / verify-locks<br/>LF-normalised SHA-256 + council hook"]
```

---

## 3. State Files, Error Handling & Fallback Contracts

| State / Module | Writer & Schema | Enforcement & Fallback Behaviour |
| --- | --- | --- |
| `.council/runs/<id>/run.json` | Tool-only ledger (`src/run/mod.rs`, `src/run/steps.rs`, `src/run/ingest.rs`). | Resumable step machine (`init` -> `next` -> `dispatch` -> `ingest` -> `cut` -> `status` -> `audit`). Rejected seat outputs move aside with a retry prompt; `--drop <seat> --reason` advances explicitly; malformed input yields plain exit code `1`/`2` errors rather than panicking. |
| `docs/ARCHITECTURE.md` & `.council/map/state.json` | `src/codemap.rs` + `src/pyread.rs` (`tree-sitter-python` + deterministic cross-language structure extractors & PageRank focus maps). | `map --check` detects structural drift while ignoring whitespace/function-body edits and `test_*` additions; never overwrites a human-authored `docs/ARCHITECTURE.md` lacking the `council-loop:map` marker. |
| `.council/memory/` or Obsidian Vault | `src/memory.rs` (`add_research`, `write_commons`, `digest`, `retire`). | Research facts require a source (recall facts are tagged accordingly) and are injected immediately; inter-seat thoughts arrive unsigned one step later and never enter blind votes. |
| `skills/council-loop/models/policy.yaml` & `lock.yaml` | `src/refresh.rs` (`council refresh`) using Artificial Analysis data + `--offline` snapshots. | Resolves `solo`, `crosslab`, and `fanout:*` across `max`/`balanced`/`efficient`; `src/providers.rs` dispatches `claude`, `codex`, `gemini`, or `OPENROUTER_API_KEY` with unreachable seats routed to stand-ins or reported cleanly in `council doctor`. |
| `.council/locks.json` | JSON map of forward-slash relative file paths -> LF-normalised SHA-256 digests (`src/core.rs`). | Tolerates CRLF, UTF-8 BOM, and UTF-16 reads while writing `\n` UTF-8 everywhere (`tests/test_portability.py`). Hook checks exit `0` defensively when no lock manifest applies so misconfigured shells never wedge unrelated tool calls. |
