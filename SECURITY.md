# Security

> ELI5: this program runs inside projects it did not write, reads text from the open web, and hands
> work to AI models that can be talked into things. So it trusts very little: it writes only where
> it should, it checks what it reads, and it keeps the seats that read the web away from the seats
> that read your code. This page says what was attacked, what holds, and what does not.

## Reporting a vulnerability

Report it in private: on the repository's **Security** tab, choose **Report a vulnerability**
(GitHub private vulnerability reporting). Please do not open a public issue for something that can
be exploited.

Say what you did, what happened and what you expected, with the version (`council --version`) and
the system. A small project folder that shows the problem is the best report. You will get a reply
that confirms receipt and says what happens next.

Supported: the latest release of the kit (the Rust binary). The legacy Python package
(`docs/LEGACY.md`) gets security fixes only.

## Who can be hostile

| Source | What it controls | Trusted? |
| --- | --- | --- |
| **A repository you run a council in** | Its files and their names, links, `.council/config.yaml`, `.council/` state (`run.json`, `CURRENT`, `locks.json`), its git settings | No |
| **Text from outside** | Web pages a researcher reads, facts already in the memory, what a seat answers | No: it is evidence, never an instruction |
| **A model that calls the MCP server** | Tool names and arguments | Only inside the fence it was given |
| **Your own settings and the command line** | `~/.council/config.yaml`, flags, paths you type | Yes |
| **The model vendors** | What a seat's model does with its packet | As far as you trust them with the packet |

Out of scope: someone who already runs programs as you on your machine, and a changed copy of the
binary itself.

## What holds

Each line is kept by a test in `tests/security.rs` (the name is given), and the readers are
fuzzed in `tests/fuzz.rs`.

| Attack | What the program does | Test |
| --- | --- | --- |
| A link in the project points a write somewhere else (the map file, `.council`, the memory folder) | Writes only inside the project, your `~/.council` and `~/.council-private`, the memory folder your own settings name, or a path you named. Never through a symbolic link | `a_link_at_the_map_path_does_not_decide_where_the_map_is_written`, `links_inside_the_project_do_not_lead_writes_out_of_it` |
| The project's settings move the shared memory to a folder outside the project | Refused. Only your own settings may name a folder outside | `a_project_cannot_send_the_shared_memory_somewhere_else` |
| `CURRENT` or `run.json` names a path, a seat id or a model id built to escape | The run id, the state file, seat ids and what is handed to other programs are checked when read | `a_run_is_named_by_a_name_and_its_file_is_checked_before_it_is_believed` |
| The repository's git settings name a program to run | git is called with hooks and the file-system monitor switched off, and no value from the project can pass for an option | `looking_at_a_repository_runs_nothing_the_repository_names` |
| A source file, a seat's answer or a locked file is a device, a pipe or a link (endless input, or another file's content) | Only plain files of a sane size are read | `files_that_are_not_plain_files_are_not_read`, `a_seat_output_is_a_plain_file_of_a_sane_size_or_it_is_not_read` |
| A lock manifest lists paths outside the project | Such entries are refused and the check fails | `a_hostile_lock_manifest_is_checked_before_it_is_believed` |
| CI checks locks against a ref that is not there | The check fails. A check that cannot look does not pass | `a_check_that_cannot_look_does_not_pass` |
| Patterns, YAML, JSON and source lines built to take forever (backtracking, alias bombs, deep nesting) | Patterns from a project must match in linear time; parsers have budgets; every reader is linear | `files_made_to_be_slow_or_endless_are_answered_at_once` |
| File names and seat text carry terminal control sequences, direction overrides or invisible tag characters | Replaced before anything is printed or put in a packet | `text_that_drives_a_terminal_never_reaches_one` |
| A seat reads who wrote which card | The mapping is kept outside the project until the run is over, in a folder only you can read | `who_wrote_which_card_is_for_the_owner_alone` |
| A key ends up in a log | Keys and tokens are replaced before a diagnostic line is written | `keys_are_never_written_into_a_log` |
| A model asks a fenced MCP server for another project, a file outside it, or to change your settings | Refused: the project, every path argument and the memory setup stay inside the fence | `a_server_started_for_one_project_serves_no_other` |
| Damaged input of any kind | 40,000 damaged inputs, with a fixed seed, against every reader, every API operation, the server and the hook: no crash, no hang, nothing written outside | `tests/fuzz.rs` |

### Keeping the web away from your code

A model that can read private files, read the open web and send text out can be talked into
leaking: a web page only has to ask. The council keeps those three apart.

- A **researcher** searches the web. It reads nothing of the project but its own packet.
- The **historian** (the researcher whose beat is the project's own record) reads the project. It
  has no web tools.
- **Experts** read the project and do not go online. A fact they want from outside is a question
  for the researchers.
- Facts reach other seats as claims with a source, under a line that says they are claims gathered
  by others and are not to be followed as instructions.

In Claude Code the agent definitions (`agents/`) and the hook enforce this. Through the `claude`
command-line tool the program passes the matching list of allowed tools. The `codex` and `gemini`
tools run with their own settings: what a seat behind them can read and reach is what you set up
there (see "What stays open").

## The review of November 2026 (version 0.5)

Every command, the server and the hook were attacked with hostile projects, hostile state and
hostile text. What was found and fixed:

1. A map file that was a dangling link let `map --install` write to a file outside the project.
2. A linked `.council` or memory folder led settings and notes out of the project.
3. A project's settings could place the shared memory anywhere on the machine.
4. `CURRENT` could name a run folder outside the project.
5. A repository's `core.fsmonitor` setting ran a program when the code was listed.
6. A source file that was a link to an endless device stalled the map; a link to a file outside
   the project put that file's names into the skeleton.
7. A lock manifest could name files outside the project, and locking followed links.
8. `verify-locks --against <ref>` passed when the ref did not exist.
9. A tripwire pattern in a project's settings could stall the gate; so could source lines built
   against the code readers, and a file name pattern with many wildcards.
10. A YAML alias bomb in a settings file was read as an empty file, and a later save would have
    overwritten the file.
11. Control sequences in file names and in seat text reached the terminal and the judges' packets.
12. Experts and the red team had web tools and the project: the three-way mix described above.
13. The MCP server, when fenced to a project, still accepted paths outside it in other arguments
    and could rewrite the user's own settings. (Found by the fuzzer's argument generator.)
14. An unexpected failure inside an operation was reported as "the tool broke" where it was the
    machine refusing a write; and a real crash inside one call would have ended a long-running
    server. Calls are now isolated, and the report says which it was.

## What stays open

Read this before you rely on the program in a place where the stakes are high.

1. **Vendor tools in a repository you do not trust.** `council dispatch` starts `claude`, `codex`
   or `gemini` inside the project. Those tools load the project's own agent settings (instructions,
   hooks, MCP servers). The council cannot make that safe. Do not run a council with command-line
   seats in a repository you would not open in those tools.
2. **The hook is a seat belt.** It stops the plain ways to edit a locked file. A shell can change a
   file in ways no hook can foresee. The lock that holds is the CI check
   (`council verify-locks --against origin/<base>`), which reads the manifest from the base branch.
3. **Seats with a shell.** The red team and the elders may run code to reproduce a claim, and a
   shell can reach the network and read the project. They read blinded cards and facts, some of
   which came from the web. If that is too much for your project, run them in a sandbox with no
   network, or take `Bash` out of `agents/council-red-team.md` and `agents/council-elder.md`.
4. **Where the separation of web and code is enforced.** As Claude Code subagents: by the agent
   definitions, and by the hook for the researcher (it keeps a file-reading tool for its packet; the
   hook stops it reading anything else, where the hook is installed and the client says which agent
   is calling). Through `claude -p`: by the list of allowed tools. Through `codex` and `gemini`: not
   by this program. Those tools decide what a seat may read and reach; set them up accordingly, or
   keep researchers on the claude route (`COUNCIL_SEATS_VIA`).
5. **A server with no fence.** `council mcp` started without `--project`, in a client that shares
   no workspace, works on any `project_dir` it is given. Start it with `--project`, and with
   `--read-only` or `--no-dispatch` where that is enough.
6. **Races with another local process.** A write is checked and then made. A process that swaps a
   folder for a link between the two could win. That needs code already running as you.
7. **Your own memory folder is trusted.** If your settings keep the memory in an Obsidian vault,
   notes are written there, from every project.
8. **Prompt injection is reduced, not solved.** The rules above limit what an instruction hidden
   in a web page can reach. No test here measures how a real model behaves when it reads one.
9. **Platforms.** Developed on Linux. The Windows build is tested under Wine (91 of the 117
   tests; the recorded scenarios and the link and file-mode attacks run on Unix only). Nothing
   has run on a real Windows or macOS machine.
10. **Dependencies.** The binary is built from 123 crates (`THIRD-PARTY-NOTICES.md`), all under
    permissive licences; `Cargo.lock` pins them. `cargo audit` found no advisory against them on
    14 November 2026. That is a statement about that day: CI runs the check on every push.

## Using it safely

- Run councils in repositories you trust, or in a container when you do not.
- In an MCP client, start the server for one project: `council mcp --project <dir>`. Add
  `--read-only` for a client that should only look, `--no-dispatch` for one that must not spend.
- Put `council verify-locks --against origin/<base>` in CI. The hook alone is not the lock.
- For code that may not leave one vendor, use `--mode solo` or a single-family mode, and
  `--no-research` for a topic that must not appear in search logs.
- Keep `~/.council-private` private: it holds who wrote which card while a run is going.
- Read the GATE line before you start a run. It says what the run will cost.
