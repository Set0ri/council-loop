# Tutorial 3: Codex, Gemini CLI, Cursor and other agents

> ELI5: the referee does not care which AI sits in the chair. Any agent that can run a command, or
> speak MCP, or read one instruction file can run a council with it.

**Time:** 15 minutes. **Needs:** the `council` binary on your PATH ([Tutorial 1](01-first-council.md))
and one AI tool.

There are three ways in. Pick by what your tool can do.

| Your tool can | Use | What you add |
| --- | --- | --- |
| Run shell commands and read an instruction file | `AGENTS.md` | One file in your repository |
| Speak MCP | The MCP server | A few lines in the tool's settings |
| Neither, but you can script around it | The API | A script that calls `council api` |

## Way 1: `AGENTS.md`

Copy [`AGENTS.md`](../../AGENTS.md) into the root of your repository. Codex, Cursor and other
tools read that file by themselves. Gemini CLI reads `GEMINI.md` by default; add the file's name
in `.gemini/settings.json` (versions with the older, flat settings use `"contextFileName"`):

```json
{"context": {"fileName": ["AGENTS.md", "GEMINI.md"]}}
```

The file tells the agent, in about fifty lines, to use `council` for big decisions and how: start
with `council init`, then do what `council next` prints and run `council ingest`, in a loop. The
longer instructions are in the program itself. An agent that wants them runs:

```console
$ council api read --args '{"uri": "skill://council-loop/SKILL.md"}'
```

**Seats without subagents.** Claude Code starts each seat as a subagent. A tool without subagents
needs another way to give every seat a fresh context, and the program provides it: `council
dispatch` runs the seats through the `claude`, `codex` or `gemini` command-line tools, whichever
are installed, or through OpenRouter with an `OPENROUTER_API_KEY`. `next` says which it is:

```console
$ council next
...
  RUN   council dispatch        # L1 via cli:claude [haiku], L2 via cli:claude [claude-fable-5-2], L3 via cli:claude [haiku], CTRL via cli:claude [claude-fable-5-2]
  THEN  council ingest
```

Check the plumbing once before you rely on it. `council doctor` lists what is installed, and
`council doctor --probe` sends one tiny prompt to each tool, because an installed tool can still be
signed out. `council dispatch --dry-run` prints the commands it would run and runs nothing.

## Way 2: the MCP server

The server is `council mcp`: the same operations as tools, over standard input and output.

**Claude Code** (already done by the plugin):

```console
$ claude mcp add council-loop -- council mcp
```

**Codex**, in `~/.codex/config.toml` (or `codex mcp add council-loop -- council mcp`):

```toml
[mcp_servers.council-loop]
command = "council"
args = ["mcp"]
```

**Gemini CLI**, in `~/.gemini/settings.json` or the project's `.gemini/settings.json`:

```json
{"mcpServers": {"council-loop": {"command": "council", "args": ["mcp"]}}}
```

**Cursor**, in `.cursor/mcp.json` in the project or `~/.cursor/mcp.json`:

```json
{"mcpServers": {"council-loop": {"type": "stdio", "command": "council", "args": ["mcp"]}}}
```

**Claude Desktop and other clients:** the same `mcpServers` entry, in that client's settings file.

What the agent then has:

- **Tools.** `council_init`, `council_next`, `council_ingest`, `council_dispatch`, `council_cut`,
  `council_status` and the rest: 22 by default. Each answer has the text the command line prints
  (the exact next step) and a structured part that says where the run stands and which seats are
  still owed.
- **`council_read`.** Hands out the skill and the reference pages, for clients that only call tools.
- **Resources.** The same documents, plus the project's runs, map, memory index and settings.
- **Two prompts.** `council_loop` (start a council on a task) and `council_resume` (pick up the
  current run).

A good first message to an agent that has the server: *"Read the council skill with council_read
(skill://council-loop/SKILL.md), then run a Lite council on: <your question>."*

### Give the server only what it needs

By default the server works on whatever project folder it is told. Narrow it:

```json
{"mcpServers": {"council-loop": {"command": "council",
  "args": ["mcp", "--project", "/path/to/your/project", "--no-dispatch"]}}}
```

| Option | Use it when |
| --- | --- |
| `--project <dir>` | Always, unless the client shares its workspace folders. The server then refuses every other folder, and every path in a call must lie inside |
| `--read-only` | The agent should look at runs, the map and the memory, and change nothing |
| `--no-dispatch` | The agent must not start seats that cost money |
| `--toolsets run` | You want a shorter tool list: 15 tools instead of 22 |
| `--deny-tools memory_setup,map` | One or two tools should not be there |

The same options can be set with `COUNCIL_MCP_*` variables or under `mcp:` in your own
`~/.council/config.yaml`. The full table: [`commands.md`](../../skills/council-loop/reference/commands.md).

## Way 3: the API

No MCP and no instruction file? Anything that can run a program can call the API: JSON in, one
JSON object out.

```console
$ council api init --args '{"ask": "Pick the queue technology for the billing pipeline", "tier": "lite", "research": 0, "mapper": "never"}'
{
 "ok": true,
 "code": 0,
 "output": "GATE tier=lite mode=fanout:claude profile=efficient loops<=1 calls~9 tokens~1.3M rel_cost=0.66 (explicit)\nMEMBERS researchers=0 (off) mapper=off memory=files:.c…",
 "run": {
  "id": "261114-191519-pick-queue-technology",
  "tier": "lite",
  "phase": "brief",
  "loop": 0,
  "max_loops": 1,
  "mode": "fanout:claude",
  "profile": "efficient",
  "dir": "<project>/.council/runs/261114-191519-pick-queue-technology",
  "done": false,
  "owed": [],
  "next": "write brief.yaml in the run folder (next shows the template), then ingest"
 }
}
```

`run.next` is the one line a program needs: what to do now. [Tutorial 6](06-build-on-the-api.md)
drives a whole council this way.

## Which seats see what, in other tools

In Claude Code the agent definitions and the hook keep the seats that read the web apart from the
seats that read your code. Seats that run through `claude -p` get the matching list of allowed
tools. Seats that run through the `codex` and `gemini` tools run with those tools' own settings:
what they may read and reach is what you set up there. If that matters for your project, keep
researchers on the claude route, or run without them (`--no-research`). More in
[`SECURITY.md`](../../SECURITY.md).

## What is tested, and what is not

The adapters for the `codex` and `gemini` tools are written from those tools' documentation and
tested against stand-ins, not against the real tools. Before you rely on them: `council doctor
--probe`, then one Lite council. If a tool does not know a model name from the picks, name one it
knows: `COUNCIL_OPENAI_MODEL`, `COUNCIL_GEMINI_MODEL`, `COUNCIL_CLAUDE_MODEL`.
