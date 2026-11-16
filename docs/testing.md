# How it is tested

> ELI5: a referee has to be right when nobody is watching. So every rule has a test, whole games
> are replayed line by line, every attack we found is kept as a test, and thousands of broken
> inputs are thrown at it on every run. No test calls a model or spends anything.

```bash
cargo test                          # everything: 117 tests
cargo test --lib                    # the unit tests only
cargo test --test scenarios         # whole runs through the real binary
cargo test --test scenarios -- lite # scenarios whose name contains "lite"
cargo test --test security
cargo test --test surfaces
cargo test --test fuzz
cargo test --release --test speed -- --nocapture    # timings, printed; the limits hold on optimised builds only
```

## The suites

| Suite | File | Tests | What it proves |
| --- | --- | ---: | --- |
| Unit | `src/**` (`mod tests`) | 67 | The rules, one at a time: blinding, the count, the cut, the gate, the YAML writer against 1,803 documents, the hook's reading of shell lines, the API's argument checks |
| Scenarios | `tests/scenarios.rs`, `tests/cassettes/` | 19 | Whole runs through the real binary, from `init` to the audit |
| Security | `tests/security.rs` | 14 | Every attack of the security review still fails (`SECURITY.md`) |
| Surfaces | `tests/surfaces.rs` | 10 | The API from a shell, the MCP server through its protocol, the hook, the first-run check, the demo, diagnostics |
| Fuzzing | `tests/fuzz.rs` | 5 | Damaged input is answered, never a crash or a hang |
| Speed | `tests/speed.rs` | 1 | What must stay fast stays within its limit |
| Notices | `tests/notices.rs` | 1 | `THIRD-PARTY-NOTICES.md` lists every crate in the binary, as `Cargo.lock` has it |

All of them start the binary that cargo just built. Each test works in its own folders under the
system's temporary folder (a project, a home, and a "victim" folder that nothing may touch) and
removes them afterwards. Set `COUNCIL_TEST_KEEP=1` to keep them and look. Nothing reads your own
`~/.council`.

## Scenarios

Two kinds.

**Recorded scenarios** are stories kept as JSON files in `tests/cassettes/`. A story is a list of
steps: a file that a person, the chair or a seat wrote; a command that was run; what the command
printed and its exit code. It ends with every file the run left behind. The test replays the story
in fresh folders with the clock frozen, and each printed line, each exit code and each final file
must be the same as recorded.

| Recording | Commands | The story |
| --- | ---: | --- |
| `lite` | 47 | A whole Lite run: brief, research, ideas, merge, red team, fuse, rank, elder, cut, final, retro |
| `full` | 51 | A Full run: a tripwire raises the tier, three loops, a shortlist vote, notes, later research |
| `lite_in_a_vault` | 49 | The same Lite run with the shared memory in an Obsidian vault |
| `trouble` | 51 | What goes wrong: a brief that is not ready, seats that answer badly or not at all, a dropped seat |
| `malformed` | 28 | Values of the wrong type wherever a model or a person can type one |
| `stop_early` | 15 | Two loops allowed, one used: the chair stops when every elder says ship |
| `direct` | 19 | The Direct tier: no council, tests first, locks proved at the end |
| `gate` | 24 | The gate: tiers, tripwires, overrides, modes, profiles |
| `memory` | 42 | The shared memory: add, find, merge, retire, import |
| `map`, `map_real_code` | 24 | The codebase map: skeleton, staleness, install, validate; several thousand lines of real code |
| `blocks` | 22 | `blind`, `tally`, `check`, `models` on their own |
| `refresh` | 25 | Model picks rebuilt from model data served by a local stand-in |
| `settings`, `slashes` | 30 | Settings files and their order of weight; paths typed with either slash |

That is 427 commands. The recordings were first made from the Python implementation the kit began
as, at the moment the Rust binary printed and wrote exactly what it did (`docs/LEGACY.md`), so
they carry that proof forward. They run on Unix; on Windows they are skipped, because recorded
paths and line ends are those of Unix.

**Played scenarios** are written in Rust and use the scripted council that also drives
`council demo` (`src/demo.rs`): a whole Lite run, a whole Full run of three loops, a run whose
every seat sits behind a command-line tool, and that same run with trouble (a tool that is signed
out, an answer in prose, a retry). They run on every system.

**The stand-in for the vendors' tools.** In the played scenarios, `claude`, `codex` and `gemini` on
the PATH are the test program itself under another name. Started that way, it reads a packet on
standard input and answers as the script would, in the output convention of the tool it stands in
for. The adapters in `src/providers.rs` run for real; no model does.

### When a change alters what the program prints

A recorded scenario fails and shows the step, the recorded lines and the new ones. If the change
was meant:

```bash
COUNCIL_ACCEPT=1 cargo test --test scenarios     # re-records what the program printed and wrote
git diff tests/cassettes                         # review it like any other change
```

Only the program's side is re-recorded. What the world wrote (briefs, seat answers) stays as it is.
A diff larger than the change you made is a finding: read it.

### Adding a scenario

Write a played one. In `tests/scenarios.rs`, build a `Stage` (a project and a private home), run
commands with `stage.council(&[...])`, write the files a chair or a seat would write with the
helpers in `council_loop::demo::script` (`brief`, `answer`, `play_seats`, `approve_pool`,
`fill_fuse`), and assert on the output, the exit code and the files. Add it to the list in `main`.

## Security tests

One test per attack, each named for the rule it keeps: `links_inside_the_project_do_not_lead_
writes_out_of_it`, `a_check_that_cannot_look_does_not_pass`, and so on. Each builds the hostile
project (a link, a damaged state file, a device where a source file should be), runs the commands
and asserts three things: the command refused with a message that says why, it was quick, and the
victim folder is still empty. A new finding becomes a new test before it is fixed.

## Fuzzing

`tests/fuzz.rs` damages valid input and feeds it to every reader: 40,000 inputs per run, from a
fixed seed, so that a failure can be replayed (the report gives the round and the state of the
generator).

| Target | How the input is damaged | Checked |
| --- | --- | --- |
| YAML, JSON from a model, file name patterns, a written map | Bytes cut, repeated, swapped; deep nesting; long filler; control characters | No crash; the whole batch within its time limit |
| Cards, ballots, a run file | Values replaced by values of the wrong kind, keys removed | No crash |
| The hook | Damaged events and shell lines of every shape | No crash |
| Every API operation that does not send | Arguments made from each operation's own schema, then some of them bent | An envelope or a refusal, never "the tool broke"; many calls must get past the argument check |
| The MCP server | The same calls as tool calls, and damaged protocol messages | A result or an error for every request |
| The run loop | A damaged `run.json` and damaged seat answers, through the real binary | Exit code 0, 1 or 2; nothing written outside the project |

The operations that send prompts to model vendors are left out on purpose: a test never spends
anyone's money.

One rule is checked a different way. The gate's built-in tripwire patterns are compiled as ASCII
patterns for speed and matched against a reading of the ask in which every word character outside
ASCII is replaced by a stand-in. A unit test compares that with the same patterns compiled over all
of Unicode, on 20,000 made-up asks: the answers must be the same on every one.

## Speed

`tests/speed.rs` times what must stay fast and fails when a limit is passed. The limits are loose
on purpose. Current figures and what keeps them: `docs/operations.md`.

## Licence notices

`THIRD-PARTY-NOTICES.md` lists every crate the binary is built from, with its licence and the
licence texts those crates ship. `tests/notices.rs` fails when the list no longer matches
`Cargo.lock`. After adding, removing or updating a dependency, make the file again and review the
diff:

```bash
COUNCIL_ACCEPT=1 cargo test --test notices
```

## Continuous integration

`.github/workflows/ci.yml` runs, on every push and pull request:

- formatting and lints (`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`);
- `cargo test --locked` on Linux, macOS and Windows;
- the build and the tests on Rust 1.85, the oldest version the crate declares;
- the dependency check against the advisory database (`cargo audit`);
- `council map --check`: the map in `docs/ARCHITECTURE.md` matches the code;
- on pull requests, `council verify-locks --against origin/<base>`.

## What is not tested

- No real model. Seats are scripted; the vendors' tools are stand-ins. Whether a council decides
  well is a question for real runs and the control arm, not for this suite.
- No real Windows or macOS machine yet. Under Wine the Windows build passes 91 tests (65 unit, 10
  surfaces, 7 security, 5 fuzzing, 4 live scenarios); 24 need a Unix system. The notices check
  cannot start `cargo` there, and the speed limits are not meant for Wine: it met all but one, the
  hook after a shell line with 1,000 locked files (627 ms against 400), because Wine reads files
  about a hundred times slower than Linux (`verify-locks` on 1,000 files: 2.2 s against 13 ms).
- How a model behaves when it reads an instruction hidden in a web page. The program limits what
  such an instruction can reach (`SECURITY.md`); it cannot test the model.
