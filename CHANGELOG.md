# Changelog

## 0.1.0 (2026-11-13)
- First release:
  - The Council Loop skill, with its protocol, context-and-memory, model-modes and research references.
  - Templates for every stage.
  - `council.py` CLI: plan, models, blind, tally, check, lock-tests, verify-locks, ask, elders, refresh.
  - `refresh_models.py`: builds the model registry from Artificial Analysis data.
  - `providers.py`: adapters for the claude, codex, gemini and OpenRouter CLIs/APIs.
  - MCP server exposing the skill as resources and prompts, plus the council tools.
  - Claude Code plugin with 3 read-only subagents and a locked-files hook.
  - AGENTS.md for other tools.
  - CI workflow and tests.
