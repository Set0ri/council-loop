# Prior art: what this kit is built on

ELI5: before building the researchers, the mapper and the shared memory, we read how the best open
projects do each job. This page says what each one does best, what we took, and what we left.

How to read it: "took the design" means the idea was written again here from the project's public
description or source. "Follows the format" means our files can be read by that tool. No source
code from any project below is copied into this repository, so no third-party notice file is
needed. Where copying would have been the natural route (parsers for many languages), the kit uses
the upstream package as an optional dependency instead.

Checked on 13 November 2026 by opening each source. Licences are as each repository showed that
day. This is not legal advice: have your own counsel confirm before you ship anything commercial.
Star counts are shown only as a sign of adoption; they are not evidence of quality.

## Codebase maps

| Project | Licence | What it does best | What we did with it |
| --- | --- | --- | --- |
| [Aider repo map](https://github.com/Aider-AI/aider/blob/main/aider/repomap.py) | [Apache-2.0](https://github.com/Aider-AI/aider/blob/main/LICENSE.txt) | Ranks code by who uses what: an edge from the file that references a name to the file that defines it, personalised PageRank, then a cut to a token budget (default 1,024 tokens, within 15%) | **Took the design.** `codemap.ranking` uses the same edges and weights: sqrt of the use count, ×10 for names the task mentions, ×10 for long snake or camel case names (8+ characters), ×0.1 for names defined in more than five files, ×50 when the using file is in focus. `codemap.focus_map` is the task-sized map |
| [tree-sitter-language-pack](https://github.com/xberg-io/tree-sitter-language-pack) | MIT | Pre-built parsers for 371 languages, with a call that returns functions, classes and signatures | **Optional dependency.** When installed, `codemap.rich_symbols` shows full signatures for Go, Rust, Java, TypeScript and the rest. Aider itself [moved from ctags to tree-sitter](https://aider.chat/2023/10/22/repomap.html) for the same reason |
| ARCHITECTURE.md convention (Kladov, 2021) and the C4 model | n/a | A short, durable map: what each part does, where it lives, the rules that hold everywhere | **Took the design.** The mapper's sections and the rule "name files, describe what stays true". Read through a secondary summary; the original post was not opened |
| Repomix, code2prompt, Gitingest | Repomix and code2prompt: MIT per a [third-party survey](https://rywalker.com/research/code-intelligence-tools); Gitingest not checked | Pack a whole repository into one file | **Left out.** A dump is not a map: a [vendor-written comparison](https://github.com/glincker/stacklit/discussions/13) puts dumps at 50k to 500k tokens for a 10k-line repository (unsourced, and written by a competitor) |
| Serena, CodeGraph, Codebase Memory MCP | mixed (Serena: GPL for the application, per the same survey) | Live symbol navigation and call graphs through a running server | **Left out.** They answer questions during coding. The council needs a document that survives without a server. One [reported result](https://rywalker.com/research/code-intelligence-tools): about 10× fewer tokens, with answer quality of 83% against 92% for plain file exploration |

Where we differ from aider, on purpose: (1) uses of a name are counted from code only, with comments
and strings removed, because the kit has no parser for most languages by default; (2) a name that
nobody else uses gets a small share of its own file's score but does not hold rank back, so an
isolated file cannot rise; (3) resolved imports are added as edges; (4) what counts as a change of
structure never depends on tree-sitter, so two machines agree on whether a map is stale.

## Research agents

| Project | Licence | What it does best | What we did with it |
| --- | --- | --- | --- |
| [Anthropic research prompts](https://github.com/anthropics/claude-cookbooks/blob/46f21f95981e3633d7b1eac235351de4842cf9f0/patterns/agents/prompts/research_subagent.md?plain=1) (cookbook) | not shown on the pages opened | The working discipline of a research subagent: a budget by difficulty (about 10 tool calls for a hard task, up to 15), short queries (under 5 words), parallel calls, a list of weak-source signs, stop on diminishing returns | **Took the design.** Rules 2, 3, 7 and 11 of `templates/packets/research.md`, in our own words. The default budgets (10 Lite, 15 Full) come from here |
| [Anthropic lead-researcher prompt](https://github.com/anthropics/claude-cookbooks/blob/46f21f95981e3633d7b1eac235351de4842cf9f0/patterns/agents/prompts/research_lead_agent.md?plain=1) and [engineering post](https://www.anthropic.com/engineering/multi-agent-research-system) | as above | How to hand out work: one objective per researcher, an output format, sources to prefer, clear boundaries, no overlap | **Took the design.** Each researcher has one beat and is told what the others cover |
| [Open Deep Research](https://github.com/langchain-ai/open_deep_research) | MIT (archived 21 August 2026) | Reflection after each search, and hard stop rules: three good sources, or two searches in a row with nothing new | **Took the design.** Rules 4 and 11, and "no acronyms in research questions". Archived, so it is a source of ideas, not a dependency |
| [GPT Researcher](https://github.com/assafelovic/gpt-researcher) | Apache-2.0 | A planner writes sub-questions; workers gather and keep the source of every summary | **Took the design.** Plan first; every fact carries its source and an exact quote |
| [STORM](https://github.com/stanford-oval/storm) | MIT | Questions asked from several discovered perspectives ([paper](https://arxiv.org/abs/2402.14207): 25% better organised, 10% broader) | **Took the design.** The researcher asks what each of the council's lenses would want to know |

What we did not take: a second search stack. GPT Researcher and Open Deep Research bring their own
search keys and scrapers. Our researchers use the web tools of the host they run in, so there is
nothing extra to install or pay for. A project that already runs one of those tools can point a
researcher seat at it: any process that writes the researcher's JSON to the output file is a researcher.

## Shared memory

| Project | Licence | What it does best | What we did with it |
| --- | --- | --- | --- |
| [Basic Memory](https://github.com/basicmachines-co/basic-memory) | AGPL-3.0 | Knowledge as local Markdown that people and agents both edit; [observations and relations](https://docs.basicmemory.com/raw/concepts/knowledge-format.md) as plain lines; opens in Obsidian | **Follows the format, uses no code.** Each research note has `## Observations` (`- [fact] ... #tag (context)`) and `## Relations` (`- found_in ...`, `- same_source ...`), so Basic Memory can index the folder as it is |
| [obsidian-skills](https://github.com/kepano/obsidian-skills) | MIT | The reference for Obsidian's formats, written for agents: Markdown, [Bases](https://github.com/kepano/obsidian-skills/blob/main/skills/obsidian-bases/SKILL.md), JSON Canvas | **Follows the format.** Properties, tags, wikilinks with the path inside the vault, and a `Research.base` table of every fact |
| [Graphiti](https://github.com/getzep/graphiti) | Apache-2.0 | "When information changes, old facts are invalidated — not deleted", and every fact traces back to its source | **Took the design.** `council memory retire` keeps the note with its reason and what replaced it |
| [Letta shared memory blocks](https://docs.letta.com/guides/agents/shared-memory-blocks) | docs | One block attached to many agents; the guide warns of races when several write at once | **Took the warning.** Seats never write the memory. They put facts and notes in their output and one script files them |
| [Claude Code agent teams](https://code.claude.com/docs/en/agent-teams) | docs | A mailbox per agent and a shared task list; direct messages between teammates | **Took the mailbox, changed the timing.** Notes are delivered one step later, unsigned, and never inside a blind vote, because the council's value depends on seats thinking alone first |
| Blackboard systems ([Salemi et al.](https://arxiv.org/abs/2510.01285v2)), A-MEM ([Xu et al.](https://arxiv.org/abs/2502.12110)) | papers | A request board apart from a response board; one idea per linked note | **Took the design.** Questions for the researchers are requests; facts are the responses; one fact per note |

What we did not take: a database. Mem0, Zep and Letta need a service or a graph store. The council
writes plain files that git can diff and a person can read. If search by keyword stops finding
things, point Basic Memory or another indexer at the same folder; nothing has to move.

## Councils and multi-agent debate

Earlier research for this kit is in `reference/research.md`: [LLM Council](https://github.com/karpathy/llm-council)
for anonymous peer ranking, and the papers behind isolated ideation, blind votes and cross-lab
judges. One addition from this round: Claude Code's own documentation notes that
"sequential investigation suffers from anchoring", which is the same reason seats here work alone
before they read each other's notes.

## If you want to copy code later

All of the projects marked MIT or Apache-2.0 above allow it, with their notice kept. Three places
where it could pay off, and what it would take:

1. Aider's tree-sitter tag queries (its `queries` folder, derived from MIT and Apache-2.0 grammars)
   would give exact definitions and references for many languages without the optional package.
   Cost: vendoring one query file per language and keeping them current.
2. The obsidian-markdown skill could ship beside this one so that agents which edit the vault by
   hand follow the same rules. It installs separately today: `npx skills add` with the repository address.
3. GPT Researcher as a researcher seat for teams that already run it. Cost: its own keys and a
   small adapter that turns its sources into the fact format.

Do not copy from Basic Memory (AGPL-3.0) into this MIT-licensed kit, and check Serena's licence before taking anything from it.
