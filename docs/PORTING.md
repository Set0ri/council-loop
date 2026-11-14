# Two implementations, one behaviour

> ELI5: the same referee, written twice. Tests make both say the same thing, word for word, so you can trust either one.

The bookkeeping of the Council Loop exists as a Rust binary, `council` (in `src/`), and as a Python
reference (in `skills/council-loop/scripts/`, `mcp/server.py` and `hooks/protect_locked_files.py`).
This page says how the two are kept the same, where they still differ, and what has not been proven.

## What maps to what

| Python reference | Rust | What it is |
| --- | --- | --- |
| `council.py` (helpers, settings, blind, tally, cut, check, locks) | `src/core.rs` | The rules |
| `council.py` (doctor, plan) | `src/gate.rs` | The gate |
| `council.py` (the command line) | `src/cli.rs` | Every verb and flag |
| `run.py` | `src/run/` (`steps`, `ingest`, `validate`, `prompts`, `seats`, `cut`, `dispatch`, `status`) | The run loop |
| `memory.py` | `src/memory.rs` | The shared memory |
| `codemap.py` | `src/codemap.rs`, `src/pyread.rs` | The codebase map |
| `providers.py` | `src/providers.rs` | Vendor tools and OpenRouter |
| `refresh_models.py` | `src/refresh.rs` | Model picks |
| `mcp/server.py` | `src/mcp.rs` | The MCP server (`council mcp`) |
| `hooks/protect_locked_files.py` | `src/hook.rs` | The test-lock hook (`council hook`) |
| the standard library and PyYAML | `src/py.rs`, `src/pyrand.rs`, `src/yaml.rs`, `src/tpl.rs`, `src/val.rs` | Python's formatting of text, numbers and JSON; CPython's shuffle; PyYAML's reader and block writer; `string.Template` |

The launcher `skills/council-loop/scripts/council` (and `bin/council`, `bin/council.cmd`) runs the
binary when it finds one and the Python reference when it does not.

## How sameness is tested

`tests/test_parity.py` plays scenarios through both implementations and compares everything.

- A scenario is a list of steps: commands, and the files a person or a seat would write in between.
- It is played once per implementation, in two fresh folders, with the clock frozen (`COUNCIL_NOW`,
  one second per command), the same settings folder (`COUNCIL_HOME`), and fake seats (`tests/sim.py`)
  or fake vendor tools (`tests/fakecli.py`), so no model is called.
- Then every line on standard output and standard error, every exit code and every file is compared.
  Files are compared byte for byte; JSON files by value.
- Three things are folded before comparing, and nothing else: the temporary folders' own paths, the
  one line of `doctor` that names the implementation (`"python": …` or `"council": …`), and the
  random scratch folder the codex adapter uses.
- For a handful of steps only the exit code is compared, because the text comes from somewhere else:
  wrong usage (argparse and clap word it differently), and syntax errors reported by the YAML or JSON
  library.
- A third play sends each command to the other implementation than the one before it, inside one
  project. It must print and write what one implementation alone does. This is what lets a run
  started by either be finished by the other.

What is covered, in numbers:

| Layer | What | Count |
| --- | --- | --- |
| Rust unit tests (`cargo test`) | The helpers, among them the YAML writer against documents as PyYAML writes them (`tests/fixtures/yaml_dump.json`) | 42 tests, 1,803 documents |
| Scenarios, each implementation alone | Gate, settings, building blocks, memory, map, the kit's own code through both readers, model refresh, paths typed with forward slashes, and eight whole runs: Lite, Lite in an Obsidian vault, Full, stopped early, Direct, a run where seats fail in every way, a run through vendor tools, a run with malformed input | 16 scenarios, 475 commands per play |
| Scenarios, alternating implementations | The eight whole runs, started by either | 16 plays |
| `tests/test_surfaces.py` | Both MCP servers through a real MCP client: the same tools, schemas, resources and prompt, and a whole Lite run through the tools. Both hooks on the same events. Both launchers (`scripts/council` and `bin\council.cmd`) and the plugin's own hook line | 5 tests (one of them only on Windows), 40 hook events |
| The reference alone | `tests/test_*.py` other than the two above | 109 tests |

Run it: `cargo build --release`, then `python3 -m pytest -q`. Without a built binary the parity and
surface tests are skipped; that is not a pass.

## Rules both follow

These are the conventions that make the output the same on every system. A change that breaks one
shows up as a parity failure.

- Text is UTF-8 and lines end in `\n`, in files and on the console, on every system. (Python's text
  mode would write `\r\n` on Windows; the reference turns that off everywhere, and a test reads the
  source to keep it so.)
- Files are read tolerantly: `\r\n`, a byte-order mark and UTF-16 from Windows tools are accepted.
- A path inside the project is stored and shown from the project's root with forward slashes. A full
  path is shown the way the system writes it.
- What a vendor tool receives on standard input is the prompt's bytes, unchanged.
- Sums that decide an order are made in a sorted order and rounded to 9 places, so a result never
  hangs on the last bit of a float.
- A seeded shuffle is CPython's (`src/pyrand.rs`), so a blind order drawn from a seed is the same.
- Every message says `council <verb>`, whichever implementation prints it.
- Wrong input from a person or a model gets a plain message and exit code 2 (the person can fix it)
  or 1 (a check failed). Neither implementation may crash on it.

## Known differences

- `doctor` names the implementation: `"council": "0.4.0 (rust)"` or `"python": "3.x.y"`.
- Wrong usage (an unknown flag, a missing argument) is worded by clap in the binary and by argparse
  in the reference. Both exit with 2.
- Where the reference still ends with a Python traceback (exit 1), the binary prints one line
  starting with `error:` (exit 1). Known case: `init` when the run folder already exists.
- Messages that come from the system or a library (a file that cannot be opened, YAML or JSON that
  does not parse, an HTTP failure) are worded differently.
- JSON with `NaN` or `Infinity` is accepted by the reference and refused by the binary.
- A YAML value that looks like a date is kept as text by the binary. The reference turns it into a
  date and writes it back as text; the result is the same except for a time written with a `T`
  (`2026-11-13T10:00:00` comes back with a space from the reference).
- The reference can use the optional `tree-sitter-language-pack` for richer symbols in languages
  other than Python. The binary has the built-in readers only. A map made with the pack may look
  changed to the binary: run `council map --accept` once. (What counts as a structural change always
  comes from the built-in readers, so the two agree on whether a map is stale.)
- The binary carries its own copy of the templates, references and model data. It uses a skill folder
  on disk when it finds one (`COUNCIL_SKILL_DIR`, or next to the binary); the reference always uses
  the folder it lives in.
- Model picks use `log2`. Its last bit can differ between systems, which could move a pick that is an
  exact tie. Not seen; `refresh` output is the same in every test here.
- Tripwire patterns in your own config are regular expressions: Python's `re` in the reference, the
  `regex` and `fancy-regex` crates in the binary. Plain patterns behave the same; exotic ones may not.

## Fixed in the Python reference along the way

Writing the second implementation, and comparing the two on hostile input, found these in the first.
Each is fixed in both and held by a parity scenario.

- A settings file whose top level is not a mapping crashed `config` and `memory`.
- A long inline JSON argument crashed `blind` and `tally` (it was tried as a file name).
- The order of a ranking could hang on the last bit of a sum made in set order: now sorted and rounded.
- `tally` crashed on a ballot that is not a mapping, a ranking that is not a list, a `best_on` that is not a mapping.
- A date typed into the brief crashed the save of `run.json`.
- A card whose title or body is not text crashed the checks; so did odd shapes in `top_risks`,
  `from`, `parents`, `responses` and `checks`.
- A brief that is not a mapping, a rubric or lenses that are not lists, and a criterion without a
  weight crashed `ingest`: now four plain messages.
- `refresh --offline` with a broken snapshot ended in a traceback; and a snapshot written by a live
  refresh could not be read back offline (`cost` against `cost_per_task`).
- The hook crashed on an event that is not a mapping, on a `tool_input` that is not one, on a
  `command` that is not text, and on a manifest of the wrong shape.
- On Windows the reference wrote `\r\n` into every file and onto the console, sent prompts to vendor
  tools with `\r\n`, and told users to edit a hooks file that no longer names `python3`. Found by
  running both implementations as Windows programs (below).
- The Python MCP server published its tool descriptions with the indentation of the source on Python
  3.10 to 3.12 (3.13 strips it). Now the same text on every version, and the same as the binary's.

## What has been run where

| Where | What was run | Result |
| --- | --- | --- |
| Linux x86-64, Rust 1.97 | Everything: `cargo test`, clippy, `pytest` with the binary | 42 unit tests pass, clippy is clean, `pytest`: 145 pass, 1 is for Windows only. The same with Python 3.10 and 3.12 (144 pass; the optional tree-sitter pack is not installed there) |
| Linux x86-64, Rust 1.85 (the declared minimum) | `cargo build --locked`, `cargo test --locked`, and the parity and surface tests against that binary | Builds; 42 unit tests pass; parity and surface tests: 36 pass, 1 is for Windows only |
| Linux, default text encoding ASCII | `pytest` again under `LC_ALL=C` | 138 pass; 8 are skipped by design there (they need file names that are not ASCII, or start this same run) |
| Windows builds under Wine 9 on Linux | `council.exe` cross-compiled for `x86_64-pc-windows-gnu`; its unit tests; the whole `pytest` suite, run by a Windows build of Python 3.12 against it (the test of `bin\council.cmd` among them) | 42 unit tests pass. `pytest`: 143 pass, 3 are skipped (git is not installed there, the POSIX launcher, the optional tree-sitter pack) |

How the Windows check was made, so that it can be repeated: Rust 1.97 and its sources from Ubuntu's
packages, the standard library built for the Windows target with `-Z build-std` (the packaged sources
drop the Windows lines of `library/std/Cargo.toml`; they were put back in a private copy), the
mingw-w64 linker, Wine 9, and a Windows build of CPython installed with `uv python install
cpython-3.12-windows-x86_64-none`. Wine lacks one call Python's `shutil.copy2` uses (`CopyFile2`);
a two-line `sitecustomize.py` made Python fall back to its portable copy. None of this is needed on
a real Windows machine: there, `cargo build --release` and `python -m pytest` are the whole recipe.

## Not proven

- Neither implementation has run on a real Windows or macOS machine. Wine is not Windows: it showed
  that the logic holds with Windows paths, line ends, `.cmd` tools and a Windows Python, not that
  every Windows API behaves the same. CI runs both systems on the first push
  (`.github/workflows/ci.yml`); until that run is green, treat a failure there as a finding.
- The launcher under Git Bash, and the plugin's hook line as Claude Code runs it on Windows, were
  not run. The hook line is written so that it cannot fail (it ends in `exit 0`): a hook that fails
  can block every tool call on Windows without Git Bash.
- `.github/workflows/release.yml` (binaries on a version tag) has never run. It also does not yet add
  the licence notices of the crates a binary contains (the dependency tree has about 130). All are
  permissive (MIT, Apache-2.0, BSD, ISC, Zlib, Unicode), and a tool such as `cargo-about` can
  collect them; do that before publishing a binary.
- No real model has run through the binary. The vendor adapters are tested against a fake tool that
  takes the same flags; the live model refresh against a local stand-in for the Artificial Analysis API.
- Two implementations are a cost: every change of behaviour is made twice. The parity suite is what
  keeps that honest.

## Working on the code

- Change behaviour in both implementations, then run `python3 -m pytest -q tests/test_parity.py`.
  A failure names the step and shows a diff: `-` is the reference, `+` is the binary.
- A new verb: a variant in `src/cli.rs`, a subparser in `council.py`, a tool in `src/mcp.rs` and
  `mcp/server.py`, and a step in a parity scenario.
- `src/py.rs` holds the helpers that behave as Python does (JSON layout, `repr`, rounding, `split`,
  `fnmatch`, paths as `pathlib` spells them). Use them instead of Rust's own formatting wherever the
  text is printed or stored.
- `src/val.rs` gives `serde_json::Value` the short reads the port uses everywhere (`at`, `s`, `list`,
  `truthy`, `show`).
- Print through `say!` and `complain!` (`src/out.rs`), never `println!`: the MCP server captures them.
- An error the person can fix is `core::fail("…")` (exit 2). Anything else is exit 1.
- Never type a literal `\uXXXX` escape into a Rust string through an editor that decodes it: write
  `\u{…}`.
