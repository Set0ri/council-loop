# Tutorial 2: a real council in Claude Code

> ELI5: you ask a question that matters. Claude sits in the chair, calls in experts who each think
> alone, and the program keeps score. You approve the brief at the start and get a written
> decision at the end. This page walks through one run and says what it costs.

**Time:** 30 minutes and up, most of it waiting for seats. **Needs:** Claude Code, the `council`
binary on your PATH ([Tutorial 1](01-first-council.md)), and tokens: a Lite council is about 12
agent calls and 1.8 million tokens on the host it was measured on.

This tutorial uses the same question as the demo, so that you can compare each step with what you
saw there. The output blocks are the program's own; the seats' answers will be your models'.

## 1. Install the plugin

```text
/plugin marketplace add <your-org>/council-loop
/plugin install council-loop@sylvan-labs
```

The plugin adds four things, and all of them start the `council` program:

| Part | What it does |
| --- | --- |
| The skill `council-loop` | Tells Claude how to be the chair |
| Six subagents | The seats: `council-expert`, `council-red-team`, `council-elder`, `council-researcher`, `council-historian`, `council-mapper` |
| The hook | Guards locked tests and keeps each seat in its lane |
| The MCP server | The same commands as tools, for sessions without a shell |

Skill only, without the plugin: copy `skills/council-loop/` to `.claude/skills/` and add
[`docs/CLAUDE.md-snippet.md`](../CLAUDE.md-snippet.md) to your CLAUDE.md. The seats then run as
general fresh-context agents.

## 2. Ask

In your project:

```text
/council-loop Pick the queue technology for the billing pipeline
```

or simply say "run a council on ...". Claude starts the run and shows you two lines before
anything is spent:

```console
$ council init "Pick the queue technology for the billing pipeline" --stakes medium --reversible yes
GATE tier=lite mode=fanout:claude profile=efficient loops<=1 calls~12 tokens~1.8M rel_cost=0.66 (chair: stakes=medium)
MEMBERS researchers=2 (papers and primary research; practice in the field; about 10 searches each) mapper=yes (no map yet at docs/ARCHITECTURE.md) memory=files:.council/memory
note: only one lab reachable: elders come from the same family (no cross-lab check). Install the codex / gemini CLIs or set OPENROUTER_API_KEY to get one
note: mode=fanout:claude and profile=efficient are the cheapest that work here (the default). More rigour costs more: --profile balanced or max. Keep a choice: `defaults:` in your ~/.council/config.yaml
```

**This is your moment to steer.** Say so in plain words and Claude passes it on:

| You say | What changes |
| --- | --- |
| "lite is fine", "full council", "direct" | The tier. A Full council is about 79 calls and 7 to 9 million tokens |
| "balanced", "max" | Stronger models (`--profile`). The GATE line shows the new relative cost |
| "cross-lab" | Elders from three labs (`--mode crosslab`); needs the codex and gemini tools, or an OpenRouter key |
| "solo" | One model in every seat: nothing goes to another vendor |
| "no research", "three researchers" | Who gathers facts first |
| "dry run" | `council plan`: the two lines, and nothing else happens |

Tripwires (migrations, auth, infrastructure, data loss, public APIs, publishing, architecture) only
raise the tier. Your word overrides the gate either way.

## 3. The brief (judgment point 1)

The program now asks the chair for the brief:

```console
[261114-191500-pick-queue-technology] tier=lite phase=brief
Write the intent brief: <project>/.council/runs/261114-191500-pick-queue-technology/brief.yaml
  Rubric first, before any ideas. Lenses by objective + constraint + method, not job titles.
  research_questions: what the 2 researchers should find out before anyone has ideas (6 at most; name things in full, no acronyms).
  If the requester is here, show them the brief (one screen) and set approved_by to their name (or "requester"). Otherwise: unattended.
THEN  council ingest
```

Claude fills in `brief.yaml` and shows it to you. It is one screen:

```yaml
restated: "Choose how billing events travel from an order to the ledger, so that no customer is charged twice."
eli5: "Pick the safest way to carry bills from the shop to the ledger."
rubric:
  - {id: R1, name: "no double charge, no lost event", weight: 40, check: judgment}
  - {id: R2, name: "cost to run", weight: 20, check: judgment}
  - {id: R3, name: "simple to operate", weight: 20, check: judgment}
  - {id: R4, name: "time to ship", weight: 10, check: judgment}
  - {id: R5, name: "order kept per account", weight: 10, check: judgment}
research_questions:
  - "How do billing systems stop a retried queue delivery from charging a customer twice?"
  - "Which queue designs keep events in order for one account?"
lenses:
  - {id: L1, name: "least that can go wrong", maximize: "reliability for every customer", respect: "the budget", method: "work back from the worst failure", evidence: "the facts on file and the code"}
  - {id: L2, name: "least to build and run", maximize: "simplicity", respect: "the stated peak load", method: "delete parts until it breaks", evidence: "the facts on file and the code"}
  - {id: L3, name: "room to grow", maximize: "throughput and replay", respect: "what the team can operate", method: "size it for ten times the load", evidence: "the facts on file and the code"}
facts: "Peak is 4,000 events per second. Last month 0.4% of retried events were billed twice."
approved_by: "requester"
```

Read the **rubric** above all. It is what every idea will be judged on, it is written before
anyone has ideas, and it is frozen for the run. If a criterion is missing or the weights are wrong,
say so now. When you approve, the chair writes your name (or `requester`) in `approved_by`. A chair
cannot approve its own brief: with nobody there it must write `unattended`, and the final says so.

## 4. Research and the map

```console
$ council ingest
brief accepted: rubric frozen for this run

[261114-191500-pick-queue-technology] tier=lite phase=research loop=1/1 calls=0/~12
Research: 2 researcher(s) (papers and primary research; practice in the field), 2 question(s). They bring back sourced facts. They do not propose and do not vote.
  Facts are filed in the shared memory: <project>/.council/memory
Mapper: 4 source files, 41 lines. The map goes to docs/ARCHITECTURE.md (no map yet at docs/ARCHITECTURE.md).
SPAWN 3 fresh-context agent(s) in ONE message (parallel). Each prompt is one line:
  RA    agent=council-researcher  model=fable  effort=low
        prompt: Read and follow <project>/.council/runs/261114-191500-pick-queue-technology/loop-1/research/RA.prompt.md
  RB    agent=council-researcher  model=haiku  effort=xhigh
        prompt: Read and follow <project>/.council/runs/261114-191500-pick-queue-technology/loop-1/research/RB.prompt.md
  MAP   agent=council-mapper  model=fable  effort=low
        prompt: Read and follow <project>/.council/runs/261114-191500-pick-queue-technology/loop-1/research/MAP.prompt.md
  (Plugin agent names are council-loop:<agent>. Any fresh-context agent that can Read and Write works.)
THEN  council ingest
```

This is the pattern of the whole run. The program wrote one packet per seat: a file that holds
the ask, the rubric, the facts, that seat's task and the format of its answer. The chair starts
the agents it lists, each with a one-line prompt and nothing more, and then runs `council ingest`.
The chair adds no words of its own, because anything it adds would nudge every seat the same way.

The researchers search the web and bring back facts, each with a source and an exact quote. They
do not read your code. The mapper reads your code and writes `docs/ARCHITECTURE.md`. Neither votes.

## 5. Ideas, the pool, the red team

`ingest` checks every answer against the rules (the right number of ideas, the length caps, no
card that names its author). An answer that breaks one is moved aside and that seat gets one
retry, with the reason. Then the run moves on, step by step, exactly as in the demo:

| Step | Who works | What the chair does |
| --- | --- | --- |
| `ideate` | Each expert alone, through its lens: 5 ideas (Lite) or at least 13 (Full), its best 2 as cards. The control arm writes its one-shot answer, which is sealed | Starts the seats |
| `merge` (judgment point 2) | The chair | Merges cards that are the same idea in `pool.json`, keeps every distinct direction, sets `"reviewed": true`. It does not judge |
| `vote` (Full only) | The experts, blind | Starts the seats. The program counts |
| `redteam` | The red team: a pre-mortem on the shortlist, each finding with a severity (S1 blocks, S2 must be answered, S3 is a note) | Starts the seat |
| `fuse` (judgment point 3) | The chair | Answers every S1 and S2 finding (fixed, rebutted with evidence, or accepted with a reason) and recombines the strongest parts into the candidates, in `candidates.json` |
| `rank` | The experts, blind: every voter reads the candidates under different labels in a different order | Starts the seats. The program counts |
| `elders` | The elders review the finalists and the hidden one-shot answer, not knowing which is which | Starts the seats, or `council dispatch` for elders behind another lab's tool |

You can leave. If the session is interrupted or its context is reset, `council status` says where
the run stands and `council next` says what to do; nothing is lost and nothing is done twice.

## 6. The cut (judgment point 4)

```console
$ council cut --eli5 "one plan won on reliability; its loose end is fixed"
```

The program applies the count, the machine checks (a candidate that failed a test ranks last
whatever the votes say) and the elders' verdicts, and keeps the survivors: one in a Lite run,
7 → 5 → 3 → 1 over the loops of a Full run.

Sometimes it stops and prints `JUDGMENT POINT`: the elders split, an elder raised a blocker, or the
leader was never reviewed. Then the chair asks you **one** question and records your answer
(`--asked "question -> answer"`). If you are away it takes the safer option, says so
(`--unattended "..."`) and puts that at the top of the final.

## 7. The final and the retro (judgment point 5)

The chair writes `final.md` for people: the answer first, an ELI5, one diagram, the evidence, what
was dropped and why, the risks, any dissent, and how you will know it worked. It reports the
control arm as it is, also when the one-shot answer was preferred. Then it writes `retro.md` with
at most three lessons. Lessons do not change anything by themselves: they reach `docs/lessons.md`
through a pull request that you review.

Ask Claude for the final, or read it yourself: `.council/runs/<run>/final.md`.

## 8. Check the run

```console
$ council audit      # counts every vote again from the ballots on disk
$ council log        # every step and every seat call, with times
$ council status     # the run on one screen
```

## What it cost

`council status` shows the number of calls made against the estimate. The token figure on the GATE
line is an estimate from one measured run on one host: about 1.3 million tokens for the nine
council seats of a Lite run, and an assumed 200,000 for each researcher and 150,000 for the mapper.
Most of a call's tokens are the agent's start-up overhead, not the packet. Measure your own and put
the numbers in `models/policy.yaml`.

Ways to spend less on a question that does not need more: `--no-research` when no outside facts
are needed, `--no-map` when no code is involved, `--research 1` for a narrow question, the Direct
tier for a small reversible change. Ways to spend more on one that does: `--profile balanced` or
`max`, `--mode crosslab`, the Full tier.

## Good habits

- Pass the ask in your own words. The elders judge against what you asked, not a summary of it.
- Approve the brief yourself when the decision matters.
- Do not ask the chair to "just pick one" in the middle. The votes are blind so that nobody,
  the chair included, tilts them.
- When the council's answer and the one-shot answer are equally good several times in a row, use a
  smaller tier for that kind of question.

Next: [Tutorial 4](04-locked-tests-and-ci.md) shows how the winner gets built with tests that no
agent can bend. Recommended practice and ten more examples:
[`how-to.md`](../../skills/council-loop/reference/how-to.md).
