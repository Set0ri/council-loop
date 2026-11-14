# Changelog

## 0.4.0 (2026-11-14)

The kit in Rust. One binary, `council`, is the command line, the MCP server and the test-lock hook.
The Python scripts stay as the reference, and tests hold the two to the same output.

- **One binary.** `cargo install --path .` builds `council` (crate `council-loop`, Rust 1.85 or
  newer). Every verb and flag of the Python command line is there, plus `council mcp` (the MCP server
  on standard input and output: 23 tools, 2 resources, 2 resource templates, 1 prompt; protocol
  revisions 2024-11-05 to 2025-11-25) and `council hook` (the Claude Code hook that blocks edits to
  locked files). Templates, references and model data are compiled in; a skill folder on disk wins.
  Nothing else is needed to run it. It starts in about 2 ms; the reference takes about 50.
- **The same behaviour, tested.** `tests/test_parity.py` plays 16 scenarios (475
  commands, among them eight whole runs) through each implementation and compares every line printed,
  every exit code and every file written. A third play alternates the two inside one project: a run
  started by either can be finished by the other at any step. `tests/test_surfaces.py` compares the
  two MCP servers through a real MCP client, the two hooks, and checks the launcher. Details, known
  differences and what is not proven: `docs/PORTING.md`.
- **Launcher.** `skills/council-loop/scripts/council` (also `bin/council` and `bin/council.cmd`) runs
  the binary when it finds one (`COUNCIL_BIN`, a build in the checkout, `PATH`) and the Python
  reference when it does not. The skill and the docs now say `council <verb>`.
- **Plugin wiring.** The hook runs through the launcher and can no longer fail: its line ends in
  `exit 0`, because on Windows without Git Bash a failing hook can block every tool call. It has a
  20 second limit. The MCP entry is `council mcp`: no shell, the same on every system; it needs the
  binary (the Python server is still there for clients that want it).
- **Faithful ports.** PyYAML's block writer (checked against 1,803 documents written by
  PyYAML), CPython's seeded shuffle, `string.Template`, Python's formatting of numbers and JSON, and
  a tree-sitter reader for Python code that lists what the reference's `ast` reader lists.
- **Hardening, both implementations.** Comparing the two on hostile input found a dozen places where
  the reference crashed: a ballot or a card of the wrong shape, a brief that is not a mapping, a
  criterion without a weight, a date typed into the brief, a broken model snapshot, a hook event that
  is not a mapping. Each now gets a plain message, the same one from both.
- **Windows.** Both implementations were built for Windows and run through the whole test suite
  under Wine, which found and fixed: the reference wrote `\r\n` into every file, onto the console
  and into the prompts it sends to vendor tools (now `\n` everywhere, as the binary writes); paths
  inside the project are shown with forward slashes on every system; paths given with forward
  slashes (what Git Bash hands a program) are spelled as the system spells them; the launcher passes
  over the Microsoft Store's `python3` placeholder. Not run on a real Windows machine yet.
- **Minimum Rust version, tested.** `rust-version = "1.85"`, with dependency versions picked to fit
  (resolver 3). Built and tested with Rust 1.85.1 and 1.97.
- **Small things.** `council --version` in both. Every message, template and written file says
  `council <verb>` (it said `council.py <verb>`). `council refresh` replaces
  `python3 scripts/refresh_models.py`. A snapshot written by a live refresh can be read back with
  `--offline`. `doctor` no longer prints the note about `python3` on Windows: the launcher and the
  hook no longer depend on that name.
- **CI.** `.github/workflows/ci.yml` builds, lints and tests both implementations on Linux, macOS and
  Windows, builds with Rust 1.85, and checks the codebase map. `release.yml` builds binaries for five
  targets when a version tag is pushed; it has not run yet.

## 0.3.0 (2026-11-13)

New members at the table, a memory they share, and settings for all of it. Built on a survey of the
best open projects for each part (`skills/council-loop/reference/prior-art.md`); no code was copied.

- **Researchers.** A `research` step before the first ideation. Two researchers by default (0 to 4),
  each with its own beat: papers, practice in the field, the case against, the project's own record.
  They return sourced facts, conflicts between sources, gaps and the searches they ran. Before a later
  loop they run again when a member asks them something. The packet's twelve rules follow Anthropic's
  research prompts, Open Deep Research, GPT Researcher and STORM.
- **Mapper.** `codemap.py` writes a model-free skeleton of the code; the mapper seat writes
  `docs/ARCHITECTURE.md` from it (purpose and philosophy, diagram, code map, interfaces, invariants,
  extending it); the tool checks every path the map names and never overwrites a hand-written map.
  It runs only when the structure changed (a new test or a change inside a function does not count),
  and the message names the files that moved. `map --check` for CI, `map --packet` and `map --install`
  to refresh the map without a council, `map --focus "<task>"` for the code one task points at.
- **Code ranking.** The method of aider's repo map, written again here: edges from the file that uses
  a name to the file that defines it, aider's weights, personalised PageRank, a token budget. Packets
  carry a ranked code list when the ask names code. Optional `tree-sitter-language-pack` for richer symbols.
- **Shared memory.** `memory.py`: one note per fact (claim, source, exact quote, date, confidence),
  kept across runs, and a commons page per run. Every seat may add `research` and `notes` to its
  output; one script files them. Facts reach every seat at once; thoughts arrive one step later,
  unsigned, never inside a blind vote. Voting seats are unnamed in the memory until the run ends.
  Facts without a source, and lines that read like instructions, are refused. Wrong facts are retired
  with a reason, not deleted.
- **Obsidian.** `memory setup --obsidian --vault <path>` (saved in the user's own settings) and
  `init --obsidian`: notes with properties, tags and wikilinks, a `Research.base` table, a copy of
  the map. Notes also follow Basic Memory's observation and relation lines. `memory import` moves
  notes between folders.
- **Settings.** Researchers and the mapper are options: `--research N`, `--no-research`,
  `--research-budget`, `--research-later`, `--map stale|always|never`, `--no-map`, `--focus-tokens`,
  or `members:` in `.council/config.yaml` and `~/.council/config.yaml`. `council.py config` shows every
  value and who set it. The gate prints a `MEMBERS` line and counts the extra calls.
- **Chair.** `note` posts to the commons or asks the researchers. `fuse-input.md` shows what the
  researchers found, where sources disagree, members' notes and open questions.
- **MCP.** `council_note`, `council_config`, `council_memory_*`, `council_map`; `council_init` and
  `council_plan` take the member settings.
- **Docs.** `reference/how-to.md` (recommended practice, ten worked examples), `reference/members-and-memory.md`,
  `reference/prior-art.md`, and this repository's own map in `docs/ARCHITECTURE.md`, checked in CI.
- **Anyone can be the requester.** The tool and the docs say "the requester" where they named one
  person. `approved_by` in the brief takes the name of the person who approved it (or `requester`), or
  `unattended`; values that name no person (`chair`, `claude`, `yes`) are refused.
- **Portable.** Every text read, write and pipe names UTF-8 (a test fails if one does not), the
  console prints UTF-8 whatever the locale, relative paths are stored with forward slashes, lock
  hashes ignore CRLF, files with a byte-order mark load (PowerShell's `>` writes UTF-16), tools are
  run by their resolved path (npm's `.cmd` files on Windows), and the code scan lists files with
  `git ls-files -z`. Whole runs are tested under a locale that is not UTF-8. `doctor` says what to
  do when Windows has no real `python3`. CI now runs Linux, macOS and Windows.
- **Not yet proven.** The new seats ran with scripted answers only. Their token cost in the gate is an
  assumption, and the Obsidian table was not opened in Obsidian. Nothing has run on Windows or macOS yet.

## 0.2.0 (2026-11-13)

The kit was reviewed by the model that runs it, and then by a Lite council of nine agents run through
the kit itself (`docs/council-run-2026-11-13-self-review.md`).

- **Run loop.** `init`, `next`, `ingest`, `dispatch`, `cut`, `status`: a resumable state machine with
  the run on disk. Prompts travel as files; seats answer with one line; the chair edits five files.
- **Vote count.** Ballots are read as head-to-heads weighted by each voter's stake in the two cards.
  A ballot gap is no opinion. A vote may not cut a card it could not judge: such cards stay on the
  shortlist, go to the elders, or survive the loop. Agreement is Kendall's tau between voters.
- **Blinding.** Authors are derived from `from` and `parents`; a kept card cannot be lost at merge.
  Every voter and elder reads the cards in its own evenly spaced order. The identity-leak check
  matches first-person talk, claimed roles and real seat ids only.
- **Control arm.** A hidden one-shot baseline is judged blind by the elders in every loop. It is
  checked at ingest, hashed, and can only be given up explicitly (`next --without-control`).
- **Retries and drops.** A rejected output is moved aside and counted once; `next` after a restart
  lists only the seats still owed; `ingest --drop <seat> --reason` replaces `--force`; `dispatch`
  stops after two rejections. Idea length, elder score gaps and small ballot gaps are warnings.
- **Judgment points.** The cut refuses to run on the count alone when the elders split, an S1 is
  open, or the winner was never reviewed by the elders: pass `--asked` or `--unattended`.
- **Record.** `deviation` logs anything done off-protocol. Runs are unblinded when they end and
  `audit` re-counts every vote from the ballots on disk.
- **Cost.** The GATE line shows `tokens~`, from measured per-call figures in `models/policy.yaml`.
- **Seats.** `doctor [--probe]`; seat routing (subagent, CLI, OpenRouter, labelled stand-in);
  `native_ids` for CLI model ids. Elders check claims by running code in Lite and in the `max` profile.
- **Locks.** `verify-locks --against <git ref>` reads the manifest from the base branch; the hook
  protects the manifest too.
- **MCP.** The server exposes the run loop (`council_init`, `council_next`, `council_ingest`, …).
- **Removed.** `ingest --force`; the v0.1 stage templates (replaced by `templates/packets/`).
- Tests: 47 (was 14), including whole runs with fake seats and a fake claude, codex and gemini CLI.

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
