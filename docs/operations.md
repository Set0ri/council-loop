# Running it day to day

> ELI5: when something goes wrong, you want to know two things: what happened in the run, and
> what the program was thinking. The run keeps a diary for the first. The program explains itself
> on request for the second. Neither ever mixes with a command's answer.

## Two kinds of record

| Record | What it holds | Where | How to read it |
| --- | --- | --- | --- |
| **The run diary** | Every step taken, every seat called and every answer rejected, with the time each took | `events.jsonl` in the run's folder | `council log` |
| **Diagnostics** | What a command decided and did: the gate's choice, the files read, the calls made | Standard error, or a file you name | `-v`, `-vv`, `COUNCIL_LOG` |

The state of a run is not a log: it is `run.json`, and `council status` reads it. The record of
the decision is `ledger.md` (one per loop) and `final.md`. `council audit` counts the votes again
from the ballots on disk.

## The run diary

```console
$ council log
run 261114-185750-pick-queue-technology · 12 event(s) · .council/runs/261114-185750-pick-queue-technology/events.jsonl
18:57:50  init       brief                  ok     15 ms
18:57:50  ingest     brief -> research      ok     6 ms
18:57:50  ingest     research -> ideate     ok     12 ms
18:57:50  ingest     ideate -> merge        ok     5 ms
...
18:57:50  cut        cut -> final           ok     1 ms
18:57:50  ingest     final -> retro         ok     1 ms
18:57:50  ingest     retro -> done          ok     5 ms
```

`council log -n 5` shows the last five events. `council log --json` prints the file as it is, one
JSON object per line, for a script:

```json
{"t": "2026-11-14T18:57:50", "event": "verb", "verb": "cut", "ok": true, "code": 0, "ms": 1, "phase": ["cut", "final"], "loop": [1, 1], "error": null}
```

| Event | When | Fields besides `t` and `event` |
| --- | --- | --- |
| `verb` | A command of the run loop finished (`init`, `next`, `ingest`, `dispatch`, `cut`, `deviation`, `note`) | `verb`, `ok`, `code`, `ms`, `phase` (before, after), `loop` (before, after), `error` (the first line, when it failed) |
| `seat_call` | `dispatch` called a seat through a vendor's tool or an API | `seat`, `step`, `provider`, `model`, `via`, `tools`, `ok`, `ms`, `prompt_chars`, `reply_chars`, `error` |
| `seat_output` | `ingest` rejected a seat's answer | `seat`, `step`, `state` (`rejected (attempt N)`), `errors` |

Seats that run as subagents of your own session are started by you, not by the program, so the
diary shows the `ingest` that accepted their answers and not the calls themselves.

Over MCP the diary is the resource `council://run/{id}/events`, and `council_log` is the tool.

## Diagnostics

Quiet by default: warnings and errors only. Ask for more:

```console
$ council -v plan "Rotate the API keys of the billing service" > /dev/null
[council info gate] tier=full (raised by tripwire(s): auth/security); mode=fanout:claude (default); profile=efficient (default) · tripwires=["auth/security"] · calls=78 · tokens=7684000

$ council -vv map --check > /dev/null
[council debug map] 7 file(s) listed in 0 ms
[council debug map] 7 file(s) read in 9 ms on 1 thread(s)
[council debug map] imports resolved in 0 ms, files ranked in 0 ms
[council debug map] compared with the map's record in 0 ms
```

| Ask with | Meaning |
| --- | --- |
| `-v` | Decisions (`info`) |
| `-vv` | Every read, write and call (`debug`) |
| `-vvv` | Everything (`trace`) |
| `--log-level <level>` or `COUNCIL_LOG=<level>` | `off`, `error`, `warn` (default), `info`, `debug`, `trace` |
| `COUNCIL_LOG_FILE=<path>` | Append the lines to this file as well |
| `COUNCIL_LOG_FORMAT=json` | One JSON object per line: `{"t", "level", "target", "msg", ...fields}` |

Three promises:

- Diagnostics go to standard error. A command's answer on standard output is never changed by
  them, so `council api status | jq` works at any level.
- Keys and tokens are replaced before a line is written: API keys, bearer tokens and the values of
  the variables that hold them.
- An MCP client gets the same lines as MCP log messages (`notifications/message`), at the level it
  sets with `logging/setLevel`, and progress messages while seats are dispatched.

## How fast it is

Measured on a two-core Linux machine with the release build. `cargo test --release --test speed --
--nocapture` prints the same table on yours, and fails when something is slower than the limit
(the limits are ten times these figures and more: the test is there to catch a change in kind, not
a slow afternoon).

| What | Time |
| --- | ---: |
| Start (`council --version`), `status` | 2 ms |
| The first-run check, once per machine | 3 ms more |
| `plan` or `init`, a project of 600 source files | 70 to 90 ms |
| The map's skeleton, 600 files | 80 ms |
| `plan` or the map on 3,000 files (600,000 lines) | about 1 s |
| `lock-tests` or `verify-locks`, 1,000 files | 13 ms |
| The hook before an edit or a shell line, 1,000 locked files | 4 ms |
| The hook after a shell line, 1,000 locked files | 7 ms |
| `memory search`, 150 facts | 7 ms |
| A whole scripted Lite council (`council demo`, 12 commands) | 0.1 s |
| A whole scripted Full council (`council demo --full`, 31 commands) | 0.35 s |
| One MCP server answering 300 calls (tool list, status, a resource) | 70 ms |
| The tool list a client carries | 22 tools, 19 KB (9 tools, 7 KB read-only) |

What keeps it fast, so a change does not undo it by accident:

- The hook reads the lock manifest and nothing else before an edit. After a shell line it compares
  each locked file's size and time with what it saw last, and hashes a file again only when one of
  them moved.
- The code is read on several threads; every reader is linear in the size of its input.
- The model data and the gate's built-in patterns are parsed once for the life of a process, which
  is what makes a long-running server answer in under a millisecond.
- The shared memory is plain files read in one pass: a search or an added fact costs time in
  proportion to the number of notes (about 50 microseconds each).

The program is not where the time or the money goes. A seat call takes a model many seconds and
tens of thousands of tokens; the GATE line says how many calls a run will make.

## When something goes wrong

| You see | It means | Do this |
| --- | --- | --- |
| `Not all seats are in yet` | Some seats of this step have not written their answer | Start the seats `next` lists (or `council dispatch`), then `council ingest` |
| `REJECTED (output 1)` | A seat's answer broke a rule; the reason is printed and the file moved aside | Start the retry line it prints. A second failure: `council ingest --drop <seat> --reason "<why>"` |
| `JUDGMENT POINT` | The elders split, an elder raised a blocker, or the leader was never reviewed | Ask the requester one question: `cut --asked "Q -> A"`. Nobody to ask: `--unattended "<the safer option>"` |
| `... is not a run file as this tool writes it` | `run.json` was edited or damaged | Restore it from version control or a backup. The message names the field. Do not edit it by hand |
| `refusing to write ...: it is a symbolic link` | Something in the project is a link where a file or folder should be | Remove the link. The program never writes through one |
| `... which is outside the project` | The project's settings point the memory elsewhere | Only your own settings may: `council memory setup --files --dir "<path>"` |
| `cannot read <ref>: git does not know that ref here` | CI did not fetch the base branch | `actions/checkout` with `fetch-depth: 0` |
| `map: ... STALE` | The code's structure moved and the map did not | `council map --packet`, or `council map --accept` if the map still holds |
| A seat behind a vendor's tool fails at once | The tool is signed out or does not know the model | `council doctor --probe`; set `COUNCIL_<VENDOR>_MODEL` to an id the tool knows |
| An MCP tool answers `... is not available` | The server was started without that group or with `--read-only` | Start it with `--toolsets` or without the restriction (`council mcp -h`) |
| An MCP tool answers `project_dir is required` | The server has no project and the client shares no workspace | Start it with `--project <dir>`, or pass `project_dir` |
| `the tool broke on this call` | A bug: the program caught its own crash | The run on disk is as the last finished step left it. Report it with the call that caused it |
| You lost your place | | `council status`, then `council next` |

Still unclear: run the command again with `-vv` and read what it did, then `council log` for what
the run did before.
