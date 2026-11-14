# How to use the Council Loop

ELI5: tell the tool what is being decided and how much it matters. It seats the right panel, tells
you what to do at each step, and keeps the notes. This page shows good habits and ten ready examples.

Every command and every output line on this page was run against a small sample project on
13 November 2026 (Python service: an entry point, a ledger and a queue). Seats were not real models
in those checks; where a real model is needed the example says so.

`C` below stands for `council`, the binary (`cargo install --path .` in the repository). Without it,
`C` is `sh <path to>/skills/council-loop/scripts/council`: that launcher runs the binary when it
finds one and the Python reference when it does not. The two print the same lines; tests hold them
to it. The lines on this page were first printed by the Python reference.

## The 60-second version

```bash
C doctor                                  # once: what can I call from here?
C init "<the ask, in the requester's own words>" --stakes medium --reversible yes
C next                                    # does exactly one thing: says what to do now
C ingest                                  # checks what came back; moves on only when it is valid
#  ...repeat next / ingest...
C cut --eli5 "<what happened this loop>"  # ends each loop
C status                                  # any time, and always after a context reset
```

`init` prints two lines before anything is spent. Read both.

```
GATE tier=lite mode=fanout:claude profile=balanced loops<=1 calls~12 tokens~1.8M rel_cost=1.86 (explicit)
MEMBERS researchers=2 (papers and primary research; practice in the field; about 10 searches each) mapper=yes (no map yet at docs/ARCHITECTURE.md) memory=files:.council/memory
```

The first line is the size of the council. The second is who else sits at the table: the
researchers, the mapper, and where the shared memory lives.

## Recommended practice

### Before a run

1. **Let the gate choose the size.** Give it your honest `--stakes` and `--reversible`. Tripwires
   (auth, migrations, public APIs, data loss) only raise the tier. A Full council is about 79 seat
   calls and several million tokens: use it when the decision deserves it.
2. **Use `plan` as a dry run.** `C plan "<ask>" [flags]` prints the same two lines and spends nothing.
   Compare two settings before you commit to one.
3. **Pass the ask verbatim.** The elders judge against the requester's own words, not your summary.
4. **Name the code.** If the task touches known files, pass `--paths`. Packets then carry a short
   ranked list of that code and what leans on it.
5. **Turn off what the task does not need.** No outside facts needed: `--no-research`. No code
   involved, or the map is fine: `--no-map`. One researcher is enough for a narrow question:
   `--research 1`.

### During a run

6. **Write the rubric before anyone has ideas**, and write research questions that would change the
   decision. Name things in full; a researcher that meets an acronym guesses.
7. **Do only what `next` prints.** Start the seats it lists, with the one-line prompt it gives. Extra
   words from you make the seats less independent.
8. **Do not read seat outputs, the elder packets or `control/`.** The tool digests them for you, and
   the one-shot baseline must stay unseen until the final.
9. **One retry, then drop by name.** `C ingest --drop RB --reason "<why>"`. A dropped researcher or
   mapper never stops a run; it is written down.
10. **Use the commons instead of side channels.** A thought for every member:
    `C note "<text>"`. A question for the researchers: `C note "<question>" --to research`.
11. **After a context reset: `C status`, then `C next`.** Nothing is lost and nothing is redone.

### After a run

12. **Report the control arm as it is.** If one model in one shot was preferred, say so and use a
    smaller tier for that kind of task next time.
13. **Open a pull request with the lessons** (`lessons.proposed.md`) and a row in `docs/outcomes.md`.
    Check the outcome at 30 and 90 days.
14. **Put your real numbers into `models/policy.yaml`** after a few runs. The researcher and mapper
    token figures there are assumptions until you measure them.

### Keeping the memory and the map healthy

15. **Commit** `.council/config.yaml`, `.council/memory/`, `.council/map/state.json` and
    `docs/ARCHITECTURE.md`. **Ignore** `.council/runs/`.
16. **A fact needs a source.** Add one by hand with `C memory add`. When a fact turns out wrong,
    retire it with the reason (`C memory retire`); do not delete the note.
17. **Check the map in CI.** `C map --check` exits 1 when the code's structure moved and the map did
    not. The check costs no tokens.
18. **Never hand-edit `run.json` or the first line of a generated map.** Notes are yours to annotate:
    the tool keeps what you add to a note's text.

### Cost and privacy

19. **Every seat call sends its packet to that model's vendor.** For code you may not share across
    vendors use `--mode solo` or a single-family fan-out.
20. **Researchers send search queries to the web.** For a confidential topic, write neutral
    questions or run with `--no-research` and add facts by hand.

## Settings in one table

| What | Flag | Config key (`members:`) | Default |
| --- | --- | --- | --- |
| How many researchers | `--research N`, `--no-research` | `research.seats` | 2 (0 to 4) |
| Searches per researcher | `--research-budget N` | `research.budget.lite` / `.full` | 10 / 15 |
| Research before later loops | `--research-later on_request\|always\|never` | `research.later_loops` | `on_request` |
| Which models research | | `research.model` (`auto`, `chair`) | `auto` |
| Your own beats | | `research.beats` (name + text) | built in |
| When the mapper runs | `--map stale\|always\|never`, `--no-map` | `map.when` | `stale` |
| Where the map lives | | `map.path` | `docs/ARCHITECTURE.md` |
| Who writes the map | | `map.model` (`chair`, `expert`) | `chair` |
| Ranked code list in packets | `--focus-tokens N` | `map.focus_tokens` | 700 (0 = none) |
| Facts shown per packet | | `digest` | 14 |
| Memory in Obsidian | `--obsidian` | `memory:` section | project files |

A flag beats the project's `.council/config.yaml`, which beats your own `~/.council/config.yaml`,
which beats the default. `C config` prints every value and who set it. `C config --example` prints
a commented file to start from.

## Ten worked examples

### 1. A decision that is hard to undo

When: architecture, auth, a data model, a public API, anything you cannot easily take back.

```bash
C init "Write an RFC for our new auth architecture" --stakes high --reversible no
```
```
GATE tier=full mode=fanout:claude profile=balanced loops<=3 calls~79 tokens~7.8M rel_cost=1.86 (chair: stakes=high)
MEMBERS researchers=2 (papers and primary research; practice in the field; about 15 searches each) mapper=yes (no map yet at docs/ARCHITECTURE.md) memory=files:.council/memory
```

What happens: two researchers and the mapper work first. Then up to three loops of 7 experts with
at least 13 ideas each, a blind vote, a red team, three elders, and a cut of 7 to 5 to 3 to 1.
Tune it: show the requester the brief before you ingest it. Install the codex and gemini
command-line tools (or set an OpenRouter key) so that the elders come from three labs.

### 2. Pick a library, vendor or tool

When: a choice between known options, where outside evidence matters.

```bash
C init "Pick the queue technology for the billing pipeline" --stakes medium --research 3
```
```
GATE tier=lite mode=fanout:claude profile=balanced loops<=1 calls~13 tokens~2.0M rel_cost=1.86 (chair: stakes=medium)
MEMBERS researchers=3 (papers and primary research; practice in the field; the case against; about 10 searches each) mapper=yes (no map yet at docs/ARCHITECTURE.md) memory=files:.council/memory
```

What happens: the third researcher covers "the case against": failures, postmortems and the best
arguments of people who chose otherwise. Their facts reach every expert, the red team, the elder
and the one-shot baseline alike, so the comparison stays fair.
Tune it: in the brief, write `research_questions` such as "Which managed queues keep order per
account at 4,000 events per second?".

### 3. A bug whose cause is unclear

When: several plausible causes, and you want them tested against each other instead of one after another.

```bash
C init "Find why the ledger sometimes writes one billing event twice" --tier lite --no-research \
       --paths src/ledger.py src/queue.py --focus-tokens 1200
```
```
GATE tier=lite mode=fanout:claude profile=balanced loops<=1 calls~10 tokens~1.4M rel_cost=1.86 (explicit)
MEMBERS researchers=0 (off) mapper=yes (no map yet at docs/ARCHITECTURE.md) memory=files:.council/memory
```

What happens: write each lens as one hypothesis ("a retry races the dedupe check", "the key is
built from a field that changes"). The red team is told to reproduce before it asserts. Packets
carry the two files you named and the code that uses them.
Tune it: record anything a seat proves with a `test: <command>` source; it lands in the shared memory.

### 4. A small change you can take back

When: clear, low stakes, reversible. No council.

```bash
C init "Rename the seen helper to was_delivered" --stakes low --reversible yes
C lock-tests tests            # after a different agent wrote the tests
C verify-locks                # before you call it done
C ingest                      # closes the run
```
```
GATE tier=direct mode=fanout:claude profile=balanced loops<=0 calls~1 tokens~n/a rel_cost=1.86 (chair: stakes=low, reversible)
```

What happens: one writer, tests first, then locked so the writer cannot bend them. Read
`docs/ARCHITECTURE.md` before touching the code; that is what the map is for.

### 5. Keep the codebase map current, without a council

When: the code's structure changed, a new person or agent is joining, or CI says the map is behind.

```bash
C map                         # writes the script-made skeleton; says whether the map is behind the code
C map --packet                # writes the mapper's packet for one fresh agent
C map --install .council/map/MAP.out.md    # validates the written map and puts it in place
C map --check                 # in CI: exit 1 when the map is missing or behind
```
```
map: docs/ARCHITECTURE.md: STALE: no map yet at docs/ARCHITECTURE.md
SPAWN one fresh-context agent (council-mapper). Its prompt is one line:
  prompt: Read and follow <project>/.council/map/MAP.prompt.md
map installed: docs/ARCHITECTURE.md
map: docs/ARCHITECTURE.md: the structure is as mapped on 2026-11-13
```

What happens: the script lists every file, its public names, who imports whom and which files the
rest leans on. The mapper reads that and a few key files, then writes the map with fixed sections
(purpose and philosophy, a diagram, code map, interfaces, invariants, how to extend it). A map that
names files that do not exist is sent back. A map a person wrote is never overwritten.
Tune it: `C map --focus "<a task>"` prints the code that matters for one task, in about 700 tokens.

### 6. Build up what you know on a topic

When: you want sourced facts on a subject, kept for later runs, more than you want a decision today.

```bash
C init "What is known about exactly-once delivery in managed queues?" --tier lite \
       --research 4 --research-budget 20 --map never
C memory add --claim "Managed queue A keeps order within one partition only" \
             --source https://example.org/queue-a/ordering --quote "order is guaranteed per partition"
C memory search "order partition"
C memory retire managed-queue-keeps-order-within-one-partition --because "the vendor changed this in version 4"
```
```
MEMBERS researchers=4 (papers and primary research; practice in the field; the case against; the project's own record; about 20 searches each) mapper=off memory=files:.council/memory
added managed-queue-keeps-order-within-one-partition
retired: managed-queue-keeps-order-within-one-partition (kept on disk; seats no longer receive it)
```

What happens: each fact becomes one note with its source, an exact quote, a date and a confidence.
The same fact found twice is kept once. A fact with no source is refused:
`error: not added: the source 'somewhere online' is not a web address, a file in this project, or 'test: <command>'`.
Tune it: give the researchers your own beats in `.council/config.yaml` (for example "Regulation").

### 7. Keep the council's memory in your Obsidian vault

When: you already think in Obsidian and want the council's facts and notes there.

```bash
C memory setup --obsidian --vault "/path/to/your/vault"     # once; saved in your own settings
C memory import --from .council/memory                      # bring over what a project already holds
C init "<ask>" --obsidian                                   # refuse to start if the vault is missing
```
```
saved to ~/.council/config.yaml (your user settings: every project on this machine)
memory: obsidian at <vault>/Council Loop/samples  (set by: user)
imported: 2 added, 0 already held, 0 refused
In Obsidian: open the vault, then the folder "Council Loop/samples". Start at index.
```

What happens: notes are plain Markdown with properties, tags and wikilinks, in a folder per
project. `Research.base` shows every fact as a table (held, by run, to re-check, retired), and the
codebase map is copied in as `architecture`. If the vault is not on the machine that runs a
council, the run says so and keeps its notes in the project; import them later.
Not checked from here: how the table renders inside Obsidian. The file follows the documented syntax.

### 8. Code you may not share across vendors

When: a private repository, a client's code, or a topic you do not want in search logs.

```bash
C init "Design the retry policy for the ledger" --tier lite --mode solo --no-research
```
```
GATE tier=lite mode=solo profile=balanced loops<=1 calls~10 tokens~1.4M rel_cost=n/a (explicit)
MEMBERS researchers=0 (off) mapper=yes (no map yet at docs/ARCHITECTURE.md) memory=files:.council/memory
```

What happens: every seat runs on the model you are already using, in fresh contexts. No packet goes
to another vendor and no search query leaves. You lose the cross-lab check; the write-up says so.
Tune it: `--mode fanout:claude` (or another single family) gives different models from one vendor.

### 9. Run unattended and review in the morning

When: a long run while nobody is at the keyboard.

```bash
C init "<ask>" --stakes high --reversible no
#  in brief.yaml:  approved_by: unattended
C cut --eli5 "<what happened>" --unattended "keep the leader; nothing irreversible"   # only at a judgment point
C status          # next morning
C audit           # re-count every vote from the ballots on disk
```
```
shared memory: files at <project>/.council/memory · this run: 6 fact(s) added, 9 note(s), 4 question(s) waiting for the researchers
seat outputs: 12 of 12 match what was ingested
control arm: sealed card intact
audit: ok
```

What happens: when the elders split, an elder raises a blocker, or the leader was never reviewed,
the tool stops and asks how it was settled. Unattended, you take the safer option and it is written
into the ledger, the final and the retro as a deviation. Nothing is smoothed over.

### 10. A tight budget

When: you want a second opinion on a medium decision at the lowest sensible cost.

```bash
C plan "Pick the queue technology for the billing pipeline" --tier lite
C plan "Pick the queue technology for the billing pipeline" --tier lite --profile efficient \
       --research 1 --research-budget 6 --map never --focus-tokens 0
```
```
GATE tier=lite mode=fanout:claude profile=balanced loops<=1 calls~12 tokens~1.8M rel_cost=1.86 (explicit)
GATE tier=lite mode=fanout:claude profile=efficient loops<=1 calls~10 tokens~1.5M rel_cost=0.66 (explicit)
MEMBERS researchers=1 (papers and practice; about 6 searches each) mapper=off memory=files:.council/memory
```

What happens: one researcher covers papers and practice together, the mapper stays home, and the
seats use the cheaper models of the efficient profile. Relative model cost falls from 1.86 to 0.66;
tokens fall less, because most of a call's tokens are overhead of the host. The token figures for
the researcher and the mapper are assumptions until a real run measures them.

## When something goes wrong

| You see | Do this |
| --- | --- |
| `Not all seats are in yet` | Start the seats it lists (or run `C dispatch`), then `C ingest` again |
| `REJECTED (output 1)` | The seat gets one retry with the reason attached. Start the retry line it prints |
| A seat failed twice or never came back | `C ingest --drop <seat> --reason "<why>"` |
| `JUDGMENT POINT` at the cut | Ask the requester one question and pass `--asked "Q -> A"`, or `--unattended "<safer option>"` |
| `the Obsidian vault ... is not a folder on this machine` | Run without `--obsidian`, or fix the path with `C memory setup` |
| `map: ... STALE` in CI | Refresh the map (example 5), or `C map --accept` if the map still describes the code |
| You lost your place | `C status`, then `C next` |
| Windows: `python3` is not found, or opens the Microsoft Store | Nothing to do: the launcher passes over that placeholder and uses `python` or `py`. Set `COUNCIL_PYTHON` to name an interpreter yourself. With the binary installed, Python is not used at all |
| Windows: the test-lock hook does nothing, or the plugin's MCP server does not start | The hook runs in Git Bash (install Git for Windows); the MCP entry needs `council.exe` on your PATH. Both are in the README under Windows |
| Windows PowerShell: a file made with `>` will not load | It loads: UTF-16 and byte-order marks are read. To write UTF-8 yourself, use `Out-File -Encoding utf8` |
