# Commands, the API and the server

> ELI5: one program with three doors. People type commands. Programs send JSON. AI tools speak MCP.
> Behind every door are the same operations, so the rules hold whichever door you use.

| Door | How | Who uses it |
| --- | --- | --- |
| Command line | `council <verb> [options]` | People, and agents that have a shell |
| API | `council api <operation> --args '{...}'` | Scripts, other programs, agents without MCP |
| MCP server | `council mcp` (standard input and output) | Claude Code, Claude Desktop, Cursor, Codex, Gemini CLI and other MCP clients |
| Hook | `council hook` | Claude Code, before and after tool calls |

`council <verb> -h` lists every option of a verb. `council` alone lists the verbs.

## Commands

| Verb | What it does | API operation |
| --- | --- | --- |
| `demo` | Plays a whole council on a made-up question with a script for every seat. No model, no cost. `--full` plays three loops; `--keep` keeps the folder | (none) |
| `doctor` | What this machine can call, the cheapest and the strongest mode. `--probe` sends one tiny prompt to each vendor tool | `doctor` |
| `plan "<ask>"` | Dry run of the gate: tier, mode, profile, models, seat calls, tokens. Spends nothing | `plan` |
| `init "<ask>"` | Starts a run. Prints the GATE and MEMBERS lines | `init` |
| `next` | Prepares the current step and says exactly what to do. Safe to repeat | `next` |
| `dispatch` | Runs the seats that sit behind a vendor's command-line tool or an API. `--dry-run` prints the commands only | `dispatch` |
| `ingest` | Checks what came back and moves on when it is valid. `--drop L3 --reason "..."` goes on without a seat | `ingest` |
| `cut --eli5 "..."` | Ends a loop: counts, applies the elders' verdicts, keeps the survivors | `cut` |
| `status` | One screen: where the run stands and what is owed. Use it after a context reset | `status` |
| `deviation "<what and why>"` | Records that the run left the protocol | `deviation` |
| `note "<text>"` | Posts to the commons; `--to research` asks the researchers | `note` |
| `audit` | Counts every vote again from the ballots on disk. Exit code 1 on a mismatch | `audit` |
| `log` | The run's diary: every step and every seat call, with times. `--json`, `-n N` | `log` |
| `config` | Every setting in force and who set it. `--example` prints a commented file | `config` |
| `memory status\|search\|add\|retire\|import\|setup` | The shared memory of sourced facts | `memory_*` |
| `map` | The codebase map: `--check`, `--packet`, `--install FILE`, `--validate`, `--accept`, `--focus "<task>"` | `map` |
| `models` | The model for every role in a mode and profile | `models` |
| `blind`, `tally`, `check` | The building blocks on their own: blind and shuffle cards, count ballots, check a run file | `blind`, `tally`, `check_run` |
| `lock-tests <paths>` | Records the hash of files agents may not edit | (command line only) |
| `verify-locks` | Fails when a locked file changed. In CI: `--against origin/<base>` | `verify_locks` |
| `ask`, `elders` | One prompt to one vendor; one blinded packet to the elders | `elder_review` |
| `refresh` | Rebuilds the model picks from Artificial Analysis data | (command line only) |
| `api`, `mcp`, `hook` | The other doors | |

Exit codes: `0` done. `1` a check failed (votes do not match, the map is behind, a locked file
changed), or the machine refused (a full disk). `2` something must be put right first: the output
says what.

## The API

```sh
council api --list                         # the operations, by group
council api --describe                     # all of it as JSON: schemas, documents, versions
council api init --describe                # one operation: input and output schema
council api init --args '{"ask": "Pick the queue for billing", "tier": "lite"}'
echo '{"uri": "council://run/current"}' | council api read --args-file -
```

Every answer is one JSON object, the envelope:

```json
{"ok": true, "code": 0, "output": "what the command line prints", "data": {}, "run": {}}
```

- `ok` and `code`: `code` is the exit code the command line gives; `ok` is `code == 0`.
- `output`: the text the command line prints. For the run loop it is the exact next step.
- `data`: the answer as structured data, for the operations that have one (`doctor`, `plan`, `log`,
  `read`, `config`, `verify_locks`, `memory_search`, `memory_status`, `models`, `blind`, `tally`,
  `check_run`, `elder_review`).
- `run`: for the run loop, where the run stands after the call: `id`, `tier`, `phase`, `loop`,
  `max_loops`, `mode`, `profile`, `dir`, `done`, `next` (one line: what to do now) and `owed`, a list
  of the current step's seats with `seat`, `role`, `via`, `agent`, `prompt`, `output` and `state`
  (`missing` or `written`). A caller can drive a whole council from `run` alone.

Rules that hold for every operation:

- Arguments are checked before anything runs: an unknown name, a wrong type or a missing required
  argument is refused, and the refusal names the arguments the operation takes.
- An operation that works on a project takes `project_dir` (an absolute path). From a shell the
  default is the folder you are in. A server has a default when it was started with `--project` or
  its client shares its workspace.
- `effect` says what an operation does to the world: `reads` (changes nothing), `writes` (changes
  files) or `sends` (may send prompts to model vendors: it costs money and the packet leaves the
  machine). `dispatch`, `elder_review` and `doctor` with `probe` send.
- A call that could not be understood exits with code 2 and one line on standard error. A call
  that was understood always answers with the envelope, also when `ok` is false.
- The API has a version (`api_version`, now `1`). It changes only when a caller written against it
  could break.

| Group | Operations |
| --- | --- |
| `run` | `doctor` `plan` `init` `next` `ingest` `dispatch` `cut` `status` `deviation` `note` `audit` `log` `read` `config` `verify_locks` |
| `memory` | `memory_search` `memory_add` `memory_status` `memory_setup` `memory_retire` `memory_import` |
| `map` | `map` |
| `blocks` | `models` `blind` `tally` `check_run` `elder_review` |

## Documents

Things to read have addresses. `read` hands one out (`council api read --args '{"uri": "..."}'`);
over MCP they are also the server's resources.

| Address | What it is |
| --- | --- |
| `skill://council-loop/SKILL.md` | How to run a council. Read this first |
| `council://reference/{name}` | These pages: `protocol`, `how-to`, `commands`, `members-and-memory`, `model-modes`, `context-and-memory`, `research`, `prior-art` |
| `council://templates/{name}` | `brief.yaml`, `final-writeup.md`, `run.example.json`, `config.example.yaml` and the seat packets: `ideate`, `vote`, `redteam`, `elder`, `control`, `research`, `map` |
| `council://models/lock` | The model for every role, mode and profile |
| `council://api` | Every operation and document, as JSON |
| `council://run/current` | The current run on one screen |
| `council://runs` | The project's runs: id, tier, step, ask |
| `council://run/{id}/status`, `/final`, `/events`, `/ledger/{loop}` | One run: its digest, its decision, its diary, a loop's record |
| `council://map` | The codebase map (`docs/ARCHITECTURE.md`) |
| `council://memory/index` | The index of the shared memory |
| `council://config` | Every setting in force and who set it |

## The MCP server

`council mcp` serves the API over standard input and output. Nothing is added and nothing is left
out: tools are the operations (named `council_<operation>`), resources are the documents, and two
prompts start a run (`council_loop`, with `task`) or pick one up (`council_resume`). A tool call
answers with the envelope as structured content and with `output` as text. The server reports
progress while seats are dispatched and sends its diagnostics as MCP log messages at the level the
client asks for.

A client's settings, for every project:

```json
{"mcpServers": {"council-loop": {"command": "council", "args": ["mcp"]}}}
```

Options. A flag beats the environment, which beats `mcp:` in your own `~/.council/config.yaml`.
A project's settings file cannot change any of them.

| Flag | Environment | In `mcp:` | Meaning |
| --- | --- | --- | --- |
| `--project <dir>` | `COUNCIL_MCP_PROJECT` | `project` | Serve this one project. `project_dir` may be left out; other projects are refused |
| `--read-only` | `COUNCIL_MCP_READ_ONLY=1` | `read_only: true` | Offer only the operations that change nothing |
| `--no-dispatch` | `COUNCIL_MCP_NO_DISPATCH=1` | `dispatch: false` | Never offer an operation that sends prompts to model vendors |
| `--toolsets <sets>` | `COUNCIL_MCP_TOOLSETS` | `toolsets` | Groups to offer: `run`, `memory`, `map`, `blocks`, or `all`. Default `run,memory,map` |
| `--tools <names>` | `COUNCIL_MCP_TOOLS` | `tools` | Offer exactly these operations |
| `--deny-tools <names>` | `COUNCIL_MCP_DENY_TOOLS` | `deny_tools` | Never offer these |
| `--output-schemas` | `COUNCIL_MCP_OUTPUT_SCHEMAS=1` | `output_schemas: true` | List every tool's output schema too |

The fence. A server that knows its project (from `--project`, or from the workspace roots its
client shares) works only there: a `project_dir` outside is refused, a file or folder named in
another argument must lie inside, and `council_memory_setup` may set the project's memory only
(`for_project: true`), never your own settings. A server with no project and no roots has no
fence: it works on whatever `project_dir` it is given. Start it with `--project` when the client is
not one you fully trust.

The tool list is what a client carries in every request. Its size: 22 tools, about 19 KB by
default; 15 tools, 14 KB with `--toolsets run`; 9 tools, 7 KB with `--read-only`; 27 tools, 23 KB
with `--toolsets all`.

## The hook

`council hook` reads a Claude Code hook event on standard input. Wire it without a shell:

```json
{"hooks": {
  "PreToolUse": [{"matcher": "Edit|Write|MultiEdit|NotebookEdit|Bash|PowerShell|Read|Grep|Glob",
    "hooks": [{"type": "command", "command": "council", "args": ["hook"], "timeout": 10}]}],
  "PostToolUse": [{"matcher": "Bash|PowerShell",
    "hooks": [{"type": "command", "command": "council", "args": ["hook"], "timeout": 20}]}]}}
```

- Before a tool runs: it denies an edit of a locked file, a shell line that plainly changes one
  (`rm`, `mv`, `sed -i`, a redirect), and locking again without the requester. It keeps council
  seats in their lane: a seat writes only its own output file, and a researcher reads only its packet.
- After a shell line: it compares the locked files with their hashes and reports a change once.
- It is a seat belt. A shell can change a file in ways no hook can foresee. The lock that holds is
  CI: `council verify-locks --against origin/<base>`.
- The requester lifts the lock for a session with `COUNCIL_ALLOW_LOCKED=1`.

## Settings and environment

| File | Whose | What it may set |
| --- | --- | --- |
| `.council/config.yaml` in a project | The project's. It travels with the repository | `members` (researchers, mapper), `tripwires`, `tripwire_paths`, `memory` (a folder inside the project only) |
| `~/.council/config.yaml` | Yours | `members` (your defaults), `memory` (a folder anywhere), `defaults` (`mode`, `profile`) and `mcp` (the table above) |

`council config` shows what is in force and who set it. `council config --example` prints a
commented file.

| Variable | Meaning |
| --- | --- |
| `COUNCIL_HOME` | Where your settings live (default `~/.council`) |
| `COUNCIL_PRIVATE_DIR` | Where a run's private files live: who wrote which card (default `~/.council-private`) |
| `COUNCIL_MEMORY_DIR`, `COUNCIL_OBSIDIAN_VAULT`, `COUNCIL_OBSIDIAN_FOLDER` | Where the shared memory lives |
| `COUNCIL_SEATS_VIA` | Force how seats start: `subagents`, `cli:claude`, `cli:codex`, `cli:gemini`, `none` |
| `COUNCIL_CLAUDE_MODEL`, `COUNCIL_OPENAI_MODEL`, `COUNCIL_GEMINI_MODEL` | A vendor's model id, when the picks name one your tool does not know |
| `OPENROUTER_API_KEY`, `AA_API_KEY` | Seats through OpenRouter; fresh model data from Artificial Analysis |
| `COUNCIL_LOG`, `COUNCIL_LOG_FILE`, `COUNCIL_LOG_FORMAT` | Diagnostics: the level, a file to append to, `json` for one object per line |
| `COUNCIL_NO_WELCOME` | `1`: no first-run check |
| `COUNCIL_SKILL_DIR` | A skill folder whose templates and model data win over the built-in ones |
| `COUNCIL_ALLOW_LOCKED` | `1`: the hook lets locked files be edited (the requester's switch) |
| `COUNCIL_NOW` | A fixed clock, for tests |

## Diagnostics and the run diary

- Diagnostics go to standard error and never mix with a command's answer. `-v` shows decisions,
  `-vv` every read, write and call; `--log-level` or `COUNCIL_LOG` names a level (`off`, `error`,
  `warn`, `info`, `debug`, `trace`; default `warn`). Keys and tokens are replaced before a line is written.
- Every run keeps a diary, `events.jsonl`, in its folder: one JSON object per step taken, seat
  called and answer rejected, with the time it took. `council log` prints it.

## What a run leaves on disk

```text
.council/
  runs/CURRENT                    the id of the current run
  runs/<id>/run.json              the state: any step can be picked up from here
  runs/<id>/brief.yaml            the question, the rubric, the lenses (the chair writes it)
  runs/<id>/loop-N/<step>/        each seat's packet (<seat>.prompt.md) and answer (<seat>.out.json)
  runs/<id>/loop-N/ledger.md      the loop's record: votes, findings, verdicts, the cut
  runs/<id>/control/              the sealed one-shot answer (do not open before the final)
  runs/<id>/final.md              the decision, written for people
  runs/<id>/retro.md              what was learned
  runs/<id>/events.jsonl          the diary
  runs/<id>/unblinded.json        who wrote what, added when the run is over
  memory/                         the shared memory: one note per fact, an index, the commons
  map/                            the skeleton of the code and what the map was checked against
  locks.json                      the hashes of the locked files
docs/ARCHITECTURE.md              the codebase map
```

While a run is going, who wrote which card is kept outside the project, in `~/.council-private`,
so that no seat can read it.
