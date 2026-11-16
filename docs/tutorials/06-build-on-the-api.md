# Tutorial 6: build on the API and the MCP server

> ELI5: everything the program can do is a named operation that takes JSON and answers with JSON.
> The command line, the MCP server and your own scripts all use the same list. So if you can call
> one operation, you can build anything on top: a bot, a dashboard, another agent's tool.

**Time:** 15 minutes. **Needs:** the `council` binary and a shell. `jq` makes the examples shorter.

## 1. The list

```console
$ council api --list
council API 1 · 27 operations · JSON in, one envelope out ({ok, code, output, data, run})

run
  doctor          sends   Look around this machine
  plan            reads   Dry-run the gate
  init            writes  Start a run
  next            writes  Prepare the current step
  ingest          writes  Check what came back
  dispatch        sends   Run the CLI and API seats
  cut             writes  End the loop
  status          reads   Where the run stands
  deviation       writes  Record a deviation
  note            writes  Post to the commons
  audit           reads   Re-count the votes
  log             reads   The run's diary
  read            reads   Read a document
  config          reads   Show the settings
  verify_locks    reads   Check the locked files

memory
  memory_search   reads   Find facts
  ...
```

The middle column is the operation's **effect**, and it is a promise: `reads` changes nothing,
`writes` changes files, `sends` may send prompts to model vendors (it costs money, and a packet
leaves the machine).

Everything a program needs to know about an operation is in its description: the arguments with
types and defaults, the effect, and the shape of the answer.

```console
$ council api note --describe | jq -c '{name, effect, cli, mcp_tool, required: .input.required, arguments: (.input.properties | keys)}'
{"name":"note","effect":"writes","cli":"council note","mcp_tool":"council_note","required":["text"],"arguments":["project_dir","run_id","text","to"]}
```

`council api --describe` prints all of it as one JSON document, with the documents (section 4)
and the API's version. Generate a client from it, or hand it to a model.

## 2. One call, one envelope

```console
$ council api init --args '{"ask": "Pick the queue technology for the billing pipeline", "tier": "lite", "research": 0, "mapper": "never"}'
{
 "ok": true,
 "code": 0,
 "output": "GATE tier=lite mode=fanout:claude profile=efficient loops<=1 calls~9 tokens~1.3M rel_cost=0.66 (explicit)\nMEMBERS researchers=0 (off) ...",
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

| Field | What it is |
| --- | --- |
| `ok`, `code` | `code` is the exit code the command line gives (0 done, 1 a check failed, 2 something must be put right first). `ok` is `code == 0`. The process exits with `code` too |
| `output` | What the command line prints. For people and for models: it says the exact next step |
| `data` | The answer as data, for operations that have one (`plan`, `models`, `tally`, `memory_search`, ...) |
| `run` | For the run loop: where the run stands after the call. `next` is one line; `owed` lists the seats of the current step |

Arguments are checked before anything runs, and the refusal tells a caller how to put it right:

```console
$ council api status --args '{"run": 1}'
error: status: run: no such argument (this operation takes: project_dir, run_id)
$ echo $?
2
```

Arguments can also come from a file or from standard input: `--args-file brief-args.json`,
`--args-file -`.

## 3. Drive a run from a script

A run is a loop of two calls. `next` prepares the step and says who must work. `ingest` checks
what came back and moves on. When `ingest` answers `ok: false`, somebody is still owed: a seat, or
the chair at one of its judgment points. This script moves a run forward until it needs someone:

```sh
#!/bin/sh
# pump.sh: move the current council run forward until it needs someone.
start_seat() {                 # replace this with your own agent
  echo "  start a fresh agent for seat $1 with the prompt: Read and follow $2"
}
while :; do
  council api next > step.json                              # prepares the step; safe to repeat
  if [ "$(jq -r .run.done step.json)" = true ]; then
    echo "done: $(jq -r .run.dir step.json)/final.md"; exit 0
  fi
  jq -r '.run.owed[] | select(.state == "missing") | "\(.seat) \(.prompt)"' step.json |
  while read -r seat prompt; do start_seat "$seat" "$prompt"; done
  council api ingest > step.json                            # checks what came back
  if [ "$(jq -r .ok step.json)" != true ]; then
    jq -r '"waiting at \(.run.phase): \(.run.next)"' step.json; exit 2
  fi
  jq -r '"\(.run.phase): \(.output | split("\n")[0])"' step.json
done
```

Run it on the run that was just started. It stops at the first thing only the chair can do:

```console
$ ./pump.sh
waiting at brief: write brief.yaml in the run folder (next shows the template), then ingest
```

Write the brief ([Tutorial 2](02-a-real-council.md) shows one) and run it again:

```console
$ ./pump.sh
ideate: brief accepted: rubric frozen for this run
  start a fresh agent for seat L1 with the prompt: Read and follow <project>/.council/runs/261114-192350-pick-queue-technology/loop-1/ideate/L1.prompt.md
  start a fresh agent for seat L2 with the prompt: Read and follow <project>/.council/runs/261114-192350-pick-queue-technology/loop-1/ideate/L2.prompt.md
  start a fresh agent for seat L3 with the prompt: Read and follow <project>/.council/runs/261114-192350-pick-queue-technology/loop-1/ideate/L3.prompt.md
  start a fresh agent for seat CTRL with the prompt: Read and follow <project>/.council/runs/261114-192350-pick-queue-technology/control/CTRL.prompt.md
waiting at ideate: start the 4 seat(s) still missing (dispatch runs the CLI and API ones), then ingest
```

That is the whole contract with a seat. Each entry of `run.owed` has:

| Field | Meaning |
| --- | --- |
| `seat`, `role` | Which seat (`L1`, `RT`, `EA`, `RA`, `MAP`, `CTRL`) and what it is |
| `prompt` | The packet: a file that holds the ask, the rubric, the facts, the task and the format of the answer |
| `output` | Where the seat must write its answer |
| `via`, `agent` | How the program would start it: as a subagent (`agent` names the kind), or through a vendor's tool, which `dispatch` does for you |
| `state` | `missing` or `written` |

Your `start_seat` gives a fresh model context one line, `Read and follow <prompt>`, and nothing
else. The seat writes `output`. Rules that matter if you build this:

- **Fresh context per seat.** A seat that has seen another seat's work is not independent.
- **Add nothing to the prompt.** The packet is complete. Words you add reach every seat alike.
- **Do not read the answers yourself** if your program is also the chair. `ingest` digests them.
- **Let the program reject.** A bad answer is moved aside and `ingest` says why; the seat gets one
  retry. After a second failure: `{"drop": "L3", "reason": "..."}` on `ingest`.
- **Stop where the chair is needed** (`brief`, `merge`, `fuse`, `cut`, `final`, `retro`). Those are
  judgment, and `run.next` says which file to write.

## 4. Documents

Things to read have addresses, and one operation hands them out:

```console
$ council api read --args '{"uri": "council://run/current"}' | jq -r .output
GATE tier=lite mode=fanout:claude profile=efficient loops<=1 calls~9 tokens~1.3M rel_cost=0.66 (explicit)
run 261114-192350-pick-queue-technology · phase ideate · loop 1/1
...
$ council api read --args '{"uri": "council://runs"}' | jq -r .output
{
  "runs": [
    {
      "id": "261114-192350-pick-queue-technology",
      "tier": "lite",
      "phase": "ideate",
      ...
```

The skill (`skill://council-loop/SKILL.md`), the reference pages, the packet templates, the model
picks, a run's ledgers, its final and its diary, the code map, the memory's index: the list is in
[`commands.md`](../../skills/council-loop/reference/commands.md) and in `council api --describe`.

## 5. The same thing over MCP

`council mcp` is the API with the Model Context Protocol around it. Nothing is added: tools are
the operations, resources are the documents. One exchange, by hand:

```console
$ printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"demo","version":"1"}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"council_status","arguments":{}}}' \
  | council mcp --project "$PWD" --read-only
```

The answer to the tool call carries the envelope twice: as text for a model to read, and as
structured content for a program (shortened here):

```json
{"jsonrpc": "2.0", "id": 2, "result": {
  "content": [{"type": "text", "text": "GATE tier=lite ...\nrun 261114-192350-pick-queue-technology · phase ideate · loop 1/1\n..."}],
  "structuredContent": {"ok": true, "code": 0, "output": "...", "run": {"phase": "ideate", "next": "start the 4 seat(s) still missing (dispatch runs the CLI and API ones), then ingest", "owed": ["..."]}},
  "isError": false}}
```

A tool carries hints that a client can use to decide what needs your approval. They come from the
operation's effect:

```json
{"name": "council_status", "title": "Where the run stands", "annotations": {"readOnlyHint": true, "idempotentHint": true, "openWorldHint": false}}
{"name": "council_note", "title": "Post to the commons", "annotations": {"destructiveHint": false, "openWorldHint": false}}
```

The server was started read-only above, so a call that would change something is refused, and the
refusal says why:

```json
{"content": [{"type": "text", "text": "council_init is not available: this server is read-only, and it writes"}], "isError": true}
```

What else the server does:

- **Workspace roots.** A client that shares its workspace folders gives the server its project and
  its fence: calls for folders outside are refused.
- **Progress.** While `council_dispatch` runs seats, a client that sent a progress token gets one
  message per seat.
- **Logging.** The client sets a level (`logging/setLevel`) and receives the program's diagnostics
  as log messages.
- **Prompts.** `council_loop` (with `task`) returns the skill, the task and its GATE line, sized
  for the server's project. `council_resume` returns where the current run stands.
- **Configuration** by flag, by environment and in your own settings: one project, read-only, no
  sending, groups of tools, single tools on or off. See [Tutorial 3](03-other-ai-tools.md).

## 6. What you can rely on

- **One list.** The command line, the API and the server are generated from one registry
  (`src/api.rs`). An operation cannot exist on one door and be missing on another.
- **A version.** `api_version` (now `1`) changes only when a caller written against the API could
  break. New operations and new optional arguments do not change it.
- **Strict input.** Unknown arguments are refused. Nothing is guessed.
- **An answer for every call.** An operation that fails answers with the envelope and `ok: false`.
  Even a bug inside one call is caught: the caller gets an error for that call, the server lives
  on, and the run on disk is as the last finished step left it.
- **The fence.** A caller that was given one project (`--project`, or workspace roots) cannot name
  another, cannot pass a path outside it, and cannot change your own settings.
- **Tested.** A whole council is run through MCP tool calls alone in `tests/surfaces.rs`, and every
  operation is called with thousands of made-up and damaged arguments in `tests/fuzz.rs`.

## Ideas for what to build

- A bot that starts a Lite council when a pull request gets a label, and posts `final.md`.
- A dashboard over `council://runs` and each run's `events`.
- A nightly job that runs `council map --check` and `council verify-locks` and opens an issue.
- Your own agent framework's seats: implement `start_seat` with it, and keep the referee.
