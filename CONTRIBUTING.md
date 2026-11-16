# Contributing

> ELI5: this program is a referee. A referee is only useful if it is the same every time and can
> show its work. So a change here comes with a test that proves it, and the documents say only what
> was run.

Thank you for looking. Bug reports, attacks that work, clearer wording and new scenarios are all
welcome. For anything that can be exploited, see [`SECURITY.md`](SECURITY.md) first: report it in
private.

## Build and test

You need Rust 1.85 or newer and a C compiler (the reader for Python code, tree-sitter, is C).

```bash
cargo build --release                        # the binary: target/release/council
cargo test                                   # 117 tests, about a minute, no model called
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo run --release --quiet -- map --check   # docs/ARCHITECTURE.md still matches the code
cargo run --release --quiet -- demo          # a whole council, scripted
```

What each suite proves and how to work with it (scenarios, recordings, fuzzing, speed limits):
[`docs/testing.md`](docs/testing.md). Where things are: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

## Rules for a change

1. **Behaviour comes with a test.** A new rule gets a unit test. A change to what a run does gets a
   scenario. A security finding becomes a test in `tests/security.rs` before it is fixed.
2. **When output changes on purpose, accept it and read the diff.** A recorded scenario fails and
   shows the old and the new lines. `COUNCIL_ACCEPT=1 cargo test --test scenarios` re-records what
   the program printed; `git diff tests/cassettes` must show your change and nothing else.
3. **One door for writes and risky reads.** Files are written through `src/safe.rs`
   (`write_text`, `append_line`, `make_dir`), which keeps writes inside the project and your own
   folders and never follows a link. Project state is read through `safe::read_own` or
   `read_capped`. A path the person typed on the command line goes through `write_named` or
   `core::read_named`. Text that will be printed or put into a packet goes through `safe::shown`.
   A value handed to another program as an argument is checked with `safe::plain_arg`.
4. **One registry for operations.** A new operation is one entry in `src/api.rs` (its name,
   arguments, effect and what it answers) and one verb in `src/cli.rs`. The MCP tool, its schema
   and its documentation follow from the entry; `src/mcp.rs` is not edited. An argument that names
   a file or folder has the kind `Path`, so that a fenced server keeps it inside its fence. An
   operation that can send a prompt to a model vendor has the effect `Sends`.
5. **Nothing a project controls may cost time without bound.** A pattern from a settings file is
   compiled with the linear engine (`safe::project_pattern`). A reader of project files is linear
   and has a size limit. Add the hostile case to `tests/security.rs` and the reader to
   `tests/fuzz.rs`.
6. **Nothing a test does may cost money.** No test calls a model. Seats are scripted
   (`src/demo.rs`), and the vendors' tools are stand-ins (`tests/scenarios.rs`).
7. **Messages say what to do next.** An error names what was wrong and the command or the edit
   that puts it right. Plain words; no jargon the reader has to look up.
8. **Keep it fast where it is called often.** The hook runs before and after tool calls, and the
   binary starts once per command. `tests/speed.rs` holds the limits; `docs/operations.md` holds
   the current figures.
9. **Dependencies are a cost.** Each one is code in the binary and a line in
   `THIRD-PARTY-NOTICES.md`. After adding, removing or updating one:
   `COUNCIL_ACCEPT=1 cargo test --test notices`, and `cargo audit`.

## Documents

- The pages in `skills/council-loop/` (the skill, `reference/`, `templates/`) are compiled into
  the binary and are what an AI reads. Keep them short, exact and free of anything that was not run.
- An output line shown in a document was printed by the program. When the program changes, run the
  command again and paste what it prints.
- Every document starts with an ELI5 line. Explain a term the first time it appears.
- The numbers in `README.md` (how many tests, timings, sizes) are checked before a release, not
  remembered.

## Releasing

1. Set the version in `Cargo.toml`, `.claude-plugin/plugin.json` and `skills/council-loop/SKILL.md`
   (`metadata.version`). Add the entry to `CHANGELOG.md`.
2. `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, `cargo audit`,
   `council map --check`, and `cargo build --locked` with Rust 1.85.
3. Refresh the model picks if they are old: `council refresh` (needs `AA_API_KEY`), and review the
   diff of `models/lock.yaml`.
4. Push a tag `v<version>`. `.github/workflows/release.yml` builds the archives for Linux, macOS
   and Windows and attaches them to the release, each with `LICENSE` and `THIRD-PARTY-NOTICES.md`.

## Licence

By contributing you agree that your contribution is licensed under the MIT licence of this
repository ([`LICENSE`](LICENSE)).
