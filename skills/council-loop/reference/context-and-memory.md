# Context, sizing, chunking and memory

ELI5: An AI's working memory is like a desk. Pile too much on it and it starts losing things. So keep
the desk clear, put details in labelled drawers (files), and keep a short notebook of lessons you
trust.

Every rule below links to its source. Numbers are from the cited work, not from our own tests.
Re-check them with your golden set.

## 1. Sizing: how big should the effort be?

| Decision | Rule in this skill | Why |
| --- | --- | --- |
| How many agents | Direct = 1 writer. Lite = 3 experts. Full = 7 experts + red team + 3 elders | Anthropic scales effort to the query: 1 agent with 3–10 tool calls for simple facts, 2–4 subagents for comparisons, 10+ for complex research ([Anthropic](https://www.anthropic.com/engineering/multi-agent-research-system)) |
| How many ideas | Full = 7 × ≥13, merged into families. Lite = 3 × 5 | Coverage grows roughly log-linearly with samples, but majority vote and reward models plateau ([Large Language Monkeys](https://arxiv.org/abs/2407.21787)). More calls can help, then hurt ([Chen et al.](https://arxiv.org/html/2403.02419v2)). So we only go wide when a rubric and verifiers can pick |
| How many loops | ≤3. Stop early on convergence or stall | Refinement gains come early (~20% average from [Self-Refine](https://arxiv.org/abs/2303.17651v2)). Google's ADK loop example caps at 3 ([Google](https://developers.googleblog.com/en/developers-guide-to-multi-agent-patterns-in-adk/)). Anthropic recommends explicit stopping conditions ([Anthropic](https://anthropic.com/engineering/building-effective-agents)) |
| How much debate | One structured red-team pass per loop, no open-ended debate | Majority voting explains most debate gains ([Debate or Vote](https://arxiv.org/html/2508.17536v2)). Debate can flip right answers to wrong ([Talk Isn't Always Cheap](https://arxiv.org/html/2509.05396v2)) |
| Token budget | The gate prints relative cost. Profiles trade quality for cost | Multi-agent runs use ~15x chat tokens, and token usage alone explained 80% of performance variance on BrowseComp ([Anthropic](https://www.anthropic.com/engineering/multi-agent-research-system)) |
| Build step size | One item per verify loop | One feature per session ([Anthropic harness](https://anthropic.com/engineering/effective-harnesses-for-long-running-agents)). "One item per loop" ([Huntley](https://ghuntley.com/ralph/)). The 16-agent compiler broke work into small pieces ([Anthropic](https://anthropic.com/engineering/building-c-compiler)) |
| Always-on instructions | CLAUDE.md section of 30 lines. Procedures live in this skill | Keep CLAUDE.md under 200 lines ([Claude Code docs](https://code.claude.com/docs/en/memory)). Models follow ~150–200 instructions with reasonable consistency ([HumanLayer](https://www.humanlayer.dev/blog/writing-a-good-claude-md)). Even top models hit only 68% at 500 instructions ([IFScale](https://arxiv.org/abs/2507.11538)) |

## 2. Managing context inside one run

1. **Assume context rot.** Across 18 models, accuracy falls as input grows, even on simple tasks, and
   distractors make it worse ([Chroma](https://www.trychroma.com/research/context-rot)). Aim for the
   *smallest set of high-signal tokens* ([Anthropic](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents)).
2. **Keep each agent 40–60% full.** Design the workflow around context: research, then plan, then
   implement, compacting at each boundary ([HumanLayer](https://humanlayer.dev/blog/advanced-context-engineering)).
3. **Fresh context per expert.** Subagents explore with many tokens but return only a 1,000–2,000-token
   summary ([Anthropic](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents)).
   Here, that summary is the idea card.
4. **Files, not telephone.** Subagents store their work in files and pass back lightweight references
   ([Anthropic](https://www.anthropic.com/engineering/multi-agent-research-system)). Use `.council/runs/<id>/`.
5. **Compact on purpose.** At each phase boundary, write `progress.md`: the goal, the approach, what
   is done, what is next ([HumanLayer](https://humanlayer.dev/blog/advanced-context-engineering); the
   [Anthropic harness](https://anthropic.com/engineering/effective-harnesses-for-long-running-agents)
   uses `claude-progress.txt`).
6. **Clear stale tool output.** Context editing alone improved an agentic search eval by 29%, and by 39%
   with the memory tool. It cut tokens by 84% in a 100-turn run
   ([Anthropic](https://claude.com/resources/articles/context-management)).
7. **Quiet tools.** Print a few lines and put "ERROR" plus the reason on one greppable line. Send
   details to log files, and pre-compute summaries ([Anthropic compiler](https://anthropic.com/engineering/building-c-compiler)).
8. **Just-in-time context.** Keep identifiers (paths, queries, URLs) in context and load the payloads
   when needed. Claude Code drops CLAUDE.md in up front, then uses glob/grep to pull files on demand
   ([Anthropic](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents)).
9. **Two-speed writing.** Iteration artifacts are drafts: Chain of Draft matched chain-of-thought
   accuracy with as little as 7.6% of the tokens ([paper](https://arxiv.org/abs/2502.18600)). Never
   compress evidence: numbers, quotes, file paths and test output stay verbatim.
10. **Order matters.** Put the brief and rubric first, the material in the middle, and the question
    last. Models use the middle of long contexts least well
    ([Lost in the Middle](https://arxiv.org/abs/2307.03172)).

### How the kit applies these rules to the chair

The chair runs for hours and is the one context that cannot be thrown away, so the kit keeps it small:

- **Prompts travel as files.** `next` writes each seat's packet to disk and prints a one-line prompt
  ("Read and follow <file>"). A Full loop puts about 25 short lines in the chair's context, not 25
  packets.
- **Seats answer with one line.** Their real output goes to a JSON file that the script reads. The
  chair reads digests: `cards.md`, `fuse-input.md`, the standing at the cut.
- **State is on disk.** `run.json` holds the run. After a context reset, `status` gives one screen
  and `next` lists only what is still owed, so nothing is redone and nothing has to be remembered.
- **Rules live in code where they can.** The chair does not count ballots, derive authors, remember
  which seat failed twice, or keep the control arm secret by willpower. Each of those was a place
  where a long-running model drifted or guessed (see `docs/council-run-2026-11-13-self-review.md`).
- **Measured.** In that run the packets were about 2k tokens each and a spawned agent cost about
  60k before reading one. Trimming packet words saves almost nothing; avoiding a respawn saves a lot.
  So the script sends a seat back only for a fault the run would act on.

## 3. Chunking and retrieval

- **Small corpus? Skip RAG.** Under ~200k tokens (~500 pages), put it all in the prompt, with prompt
  caching ([Anthropic Contextual Retrieval](https://anthropic.com/news/contextual-retrieval)).
- **Code: search like an engineer.** Use grep/glob/read before reaching for embeddings. This avoids
  stale indexes ([Anthropic](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents)).
- **Large corpus: contextual retrieval.**
  - Keep chunks to a few hundred tokens and prepend a short context line to each one.
  - Combine embeddings with BM25, then rerank. Retrieval failures fell 35% (contextual embeddings),
    49% (+ BM25) and 67% (+ reranking).
  - Retrieve ~150 candidates and rerank to ~20. Test on your own data
    ([Anthropic](https://anthropic.com/news/contextual-retrieval)).
- **Chunk on structure.** Split at headings, functions and sections, and keep the path or ID on
  every chunk so agents can cite it and reopen it.
- **For the council:** experts get a digest plus pointers, not the full corpus. The facts ledger keeps
  verbatim quotes with source pointers.

## 4. Memory across runs

| Layer | Holds | Size rule | Who writes |
| --- | --- | --- | --- |
| `CLAUDE.md` / `AGENTS.md` | Always-true rules and pointers | < 200 lines; council section 30 | Humans, via PR |
| Skill (`SKILL.md` + `reference/`) | Procedures | SKILL.md < 500 lines; details load on demand | Humans |
| Claude Code auto memory | Preferences and corrections | First 200 lines / 25 KB of `MEMORY.md` load each session | Claude |
| Run ledger `.council/runs/<id>/` | One run's state: brief, ideas, votes, reviews | ≤1 page per loop | Agents |
| Shared memory, research (`.council/memory/research/` or an Obsidian vault) | Facts, one per note: claim, source, exact quote, date, confidence | A packet carries at most 14 (the run's own first, then older ones that match the brief); the rest is one file read away | Only the script, from what seats and the chair report |
| Shared memory, commons (`…/commons/<run>.md`) | What members said to each other in one run | Ten most recent notes per packet, unsigned | Only the script |
| Codebase map `docs/ARCHITECTURE.md` | What the code is, how it is laid out, the rules that hold | 600 to 2,000 words; rewritten only when the structure changes | The mapper seat, checked by the script |
| Lessons `docs/lessons.md` | Reusable lessons with helpful/harmful counters | Itemized; prune when harmful > helpful | Agents propose, humans merge |
| Outcome ledger `docs/outcomes.md` | Predictions vs results at 30/90 days | One row per decision | Humans + agents |

Rules:
1. **Append itemized deltas, never rewrite the playbook.** Whole-context rewrites cause "brevity bias"
   and "context collapse". Itemized bullets with helpful/harmful counters fixed this, +10.6% on agent
   tasks ([ACE](https://arxiv.org/html/2510.04618v1)).
2. **Reflect after failures.** A short written reflection, stored and reused, lifted HumanEval to 91%
   ([Reflexion](https://arxiv.org/abs/2303.11366v4)).
3. **Gate memory changes on evals.** Version prompts and playbooks. Promote a change only when it beats
   the current version ([OpenAI self-evolving agents](https://developers.openai.com/cookbook/examples/partners/self_evolving_agents/autonomous_agent_retraining);
   [GEPA](https://arxiv.org/html/2507.19457v1)).
4. **A human holds the pen.** Lessons arrive as PRs ([compound engineering](https://every.to/guides/compound-engineering)).
   The Claude Code team adds to CLAUDE.md whenever Claude makes a mistake
   ([Cherny](https://venturebeat.com/technology/the-creator-of-claude-code-just-revealed-his-workflow-and-developers-are)).
5. **Memory is context, not enforcement.** Anything that must happen goes in a hook or CI, not in a
   memory file ([Claude Code docs](https://code.claude.com/docs/en/memory)). Never store secrets in memory.
6. **A fact carries its source, and one writer files it.** Several agents writing one store at once
   race each other ([Letta](https://docs.letta.com/guides/agents/shared-memory-blocks)). Here seats
   report and one script files. A fact that turns out wrong is retired with its reason, not deleted
   ([Graphiti](https://github.com/getzep/graphiti)).
7. **Shared thoughts must not cost independence.** Facts reach every seat at once; thoughts arrive
   one step later, unsigned, and never inside a blind vote. A request board kept apart from the
   response board "prevents cross-influence among sub-agents" ([Salemi et al.](https://arxiv.org/abs/2510.01285v2)).
8. **A map is ranked and budgeted, not dumped.** Give an agent the names the rest of the code leans
   on, within a token budget ([aider's repo map](https://aider.chat/2023/10/22/repomap.html): 1k tokens
   by default). Details: `reference/members-and-memory.md`.
6. **Where it applies.** Long-running API agents use the memory tool plus context editing
   ([Anthropic](https://claude.com/resources/articles/context-management)). Claude Code uses CLAUDE.md,
   auto memory and this skill.
