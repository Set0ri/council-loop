# The Python implementation (legacy)

> ELI5: the referee was first written in Python, then written again in Rust. For a while both were
> kept and made to agree word for word. Now the kit is the Rust program alone, and the Python
> scripts are kept apart for anyone who still needs them.

## What changed, and when

| Version | The kit |
| --- | --- |
| 0.1 to 0.3 | Python scripts only |
| 0.4 | A Rust binary and the Python scripts, held to the same output by a test suite |
| 0.5 and later | The Rust binary alone |

From 0.5 the kit holds no Python: no scripts, no launcher, no package to install. One program,
`council`, is the command line, the API, the MCP server and the hook.

## Where the Python implementation lives now

In a separate package, `council-loop-python-legacy`, version 0.4.1. It is version 0.4.0 of the
scripts plus the security fixes that came out of the review for 0.5. It is frozen: it gets security
fixes and nothing else. Its own README says how to run and test it, and how to move to this kit.

Use it only where a binary cannot be run.

| | Legacy Python 0.4.1 | This kit |
| --- | --- | --- |
| Needs | Python 3.10 or newer, PyYAML; the `mcp` package for the server | Nothing |
| The API (`council api`), documents, prompts, server options | No | Yes |
| Diagnostics, the run diary, `council log` | No | Yes |
| `council demo`, the first-run check | No | Yes |
| Default mode and profile | The strongest mode in reach, `balanced` | The cheapest mode in reach, `efficient` |
| The hook | Before a tool runs | Before and after; it also keeps seats in their lane |
| Who may search the web | Experts, the red team and researchers | Researchers only |

## Why one implementation

- **One file to install.** Nothing to set up, no interpreter, no packages, the same on every system.
- **Fast enough to sit in a hook.** The binary starts in about 2 ms; the scripts took about 50.
- **One thing to keep safe.** Every fix had to be made twice. The security review for 0.5 changed
  how files are written and read throughout; doing that in two languages doubles the places a
  mistake can hide.
- **Nothing was lost.** What the two implementations agreed on at 0.4 is kept as recordings, as
  the next section says.

## How the proof was carried over

At 0.4.0 a parity suite played 16 scenarios (475 commands, eight whole council runs among them)
through both implementations, in fresh folders with the clock frozen, and compared every printed
line, every exit code and every file written. They agreed.

Before the scripts left the kit, those scenarios were recorded from that agreed behaviour: what a
person or a seat wrote between commands, each command, and what was answered. The recordings are in
`tests/cassettes/` and are replayed by `tests/scenarios.rs` on every test run (`docs/testing.md`).
So the binary is still held to what both implementations did at 0.4, and every change to that
since then is a reviewed diff of a recording.

The parity suite itself, the recorder and the script that made the YAML test data are kept in the
legacy package, in its `history/` folder, as a record of how this was done.

## Moving from the Python scripts

- Install `council` (README, "Install") and take the old `bin/` folder off your `PATH`.
- Shared memory notes, `.council/locks.json`, the codebase map and settings files are read by both.
  Nothing has to be converted.
- Finish a run with the version that started it.
- The defaults changed: a run now takes the cheapest mode in reach and the `efficient` profile. To
  keep the old behaviour, put `defaults: {profile: balanced}` in your `~/.council/config.yaml`, or
  pass `--profile` and `--mode`.
- The plugin's hook and MCP entry now start `council` directly. If they do nothing, `council` is
  not on the `PATH` your client sees.
