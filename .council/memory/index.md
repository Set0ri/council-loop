---
type: "index"
tags: ["council/index"]
updated: "2026-11-16"
---

# Council memory

Shared by every council run in this project. Written only by `council`; safe to read, link and tag.

- **Research** (45 notes): one sourced fact per note, in `research/`.
- **Commons** (1 run): what members said to each other, in `commons/`.
- **Codebase map**: `docs/ARCHITECTURE.md` in the project.

## Newest research

- [Voting and ranking phases require zero tools under tools_for, confirming pure REST dispatch is safe for ballots.](research/voting-ranking-phases-require-zero-tools-under-tools.md) (high, n.d.)
- [Prefix stripping on Windows PathBuf is case-sensitive, permitting hook bypasses on letter-case mismatches.](research/prefix-stripping-windows-pathbuf-case-sensitive-permitting-hook.md) (high, n.d.)
- [In the measured v0.2 Lite run, rank vote alone consumed 193k tokens across three ballots due to subagent context overhead.](research/measured-v0-lite-run-rank-vote-alone-consumed.md) (high, n.d.)
- [Lite council rank votes consumed 192,926 tokens across 3 ballots (~64k per ballot) for a ~2,000-token packet due to subagent startup overhead.](research/lite-council-rank-votes-consumed-192-926-tokens.md) (high, n.d.)
- [Identity leaks during ideation produce warnings for chair merge rather than triggering seat respawns.](research/identity-leaks-during-ideation-produce-warnings-chair-merge.md) (high, n.d.)
- [council-loop MCP tools/list payload ranges from 19 KB to 23 KB across 22 to 27 tool schemas.](research/council-loop-mcp-tools-list-payload-ranges-19.md) (high, n.d.)
- [Ballot voting and ranking phases require zero tools and can be executed via pure HTTP completions without subagent tool scaffolding.](research/ballot-voting-ranking-phases-require-zero-tools-executed.md) (high, n.d.)
- [A vendor-written comparison puts full-dump tools such as Repomix at 50k to 500k tokens for a 10k-line repository and aider's repo map at about 1k; the figures are unsourced](research/vendor-written-comparison-puts-full-dump-tools-such.md) (low, 2026-04-13)
- [The tree-sitter-language-pack Python package is MIT licensed and ships pre-compiled grammars for 371 languages](research/tree-sitter-language-pack-python-package-mit-licensed.md) (high, n.d.)
- [STORM's paper reports articles judged more organized by a 25 percent absolute increase and broader in coverage by 10 percent than an outline-driven baseline](research/storm-paper-reports-articles-judged-organized-25-percent.md) (high, 2024-02-22)
- [STORM is MIT licensed and drives research by asking questions from several discovered perspectives; Co-STORM adds a mind map of what was collected](research/storm-mit-licensed-drives-research-asking-questions-several.md) (high, n.d.)
- [Open Deep Research tells a researcher to stop when it has three or more relevant sources or when its last two searches returned similar information](research/open-deep-research-tells-researcher-stop-three-relevant.md) (high, n.d.)
- [Open Deep Research has its researcher reflect after each search on what it found, what is missing and whether to go on](research/open-deep-research-researcher-reflect-after-search-found.md) (high, n.d.)
- [Open Deep Research forbids acronyms and abbreviations in the questions a lead hands to a researcher](research/open-deep-research-forbids-acronyms-abbreviations-questions-lead.md) (high, n.d.)
- [Obsidian stores a note's properties as YAML at the top of the file](research/obsidian-stores-note-properties-yaml-top-file.md) (high, n.d.)
- [The obsidian-skills repository by Obsidian's CEO is MIT licensed and documents Obsidian Markdown, Bases and JSON Canvas for agents](research/obsidian-skills-repository-obsidian-ceo-mit-licensed-documents.md) (high, n.d.)
- [An Obsidian Bases file is YAML with the top-level keys filters, formulas, properties, summaries and views, and a view is a table, cards, list or map](research/obsidian-bases-file-yaml-top-level-keys-filters.md) (high, n.d.)
- [A-MEM organises agent memory as Zettelkasten-style notes with structured attributes and links that are updated as new notes arrive](research/mem-organises-agent-memory-zettelkasten-style-notes-structured.md) (medium, 2025-02)
- [A March 2026 paper on Codebase-Memory reports about 10 times fewer tokens and 2.1 times fewer tool calls than file exploration, with answer quality of 83 percent against 92 percent](research/march-2026-paper-codebase-memory-reports-10-times.md) (medium, 2026-09-15)
- [LLM judges favour certain option labels, and the bias comes mainly from the tokens used as labels](research/llm-judges-favour-certain-option-labels-bias-comes.md) (high, 2024)
- [Letta lets one memory block be attached to several agents, and its guide warns of race conditions when agents update it at the same time](research/letta-lets-one-memory-block-attached-several-agents.md) (high, n.d.)
- [LangChain's Open Deep Research is MIT licensed, scored 0.4344 on Deep Research Bench (sixth place) and was archived read-only on 21 August 2026](research/langchain-open-deep-research-mit-licensed-scored-4344.md) (high, 2026-08-21)
- [The kit's own Lite run used 9 seat calls and 1,269,206 tokens in total](research/kit-own-lite-run-used-seat-calls-269.md) (high, 2026-11-13)
- [Graphiti keeps a validity window on every fact and invalidates an outdated fact instead of deleting it](research/graphiti-keeps-validity-window-fact-invalidates-outdated-fact.md) (high, n.d.)
- [GPT Researcher is Apache-2.0 licensed and splits the work between a planner that writes research questions and execution agents that gather sourced summaries](research/gpt-researcher-apache-licensed-splits-work-planner-writes.md) (high, n.d.)
- [Claude Code's documentation says that investigating one theory at a time biases later investigation toward the first theory](research/claude-code-documentation-says-investigating-one-theory-time.md) (high, n.d.)
- [Claude Code agent teams give each agent a mailbox file and a shared task list, are experimental, and use significantly more tokens than one session](research/claude-code-agent-teams-give-agent-mailbox-file.md) (high, n.d.)
- [The blackboard paper reports 13 to 57 percent relative improvement in end-to-end task success over baselines](research/blackboard-paper-reports-13-57-percent-relative-improvement.md) (medium, 2025-10)
- [A blackboard design for LLM agents posts requests on a shared board and collects replies on a separate one, which keeps helper agents from influencing each other](research/blackboard-design-llm-agents-posts-requests-shared-board.md) (high, 2025-10)
- [Basic Memory stores agent knowledge as local Markdown notes whose observations and relations follow fixed line patterns, and Obsidian can open the same folder](research/basic-memory-stores-agent-knowledge-local-markdown-notes.md) (high, n.d.)
- [Basic Memory is licensed under AGPL-3.0](research/basic-memory-licensed-under-agpl.md) (high, n.d.)
- [Anthropic's research subagent prompt tells the agent to stop when results stop improving](research/anthropic-research-subagent-prompt-tells-agent-stop-results.md) (high, n.d.)
- [Anthropic's research subagent prompt tells the agent to keep search queries under 5 words and never to repeat the same query](research/anthropic-research-subagent-prompt-tells-agent-keep-search.md) (high, n.d.)
- [Anthropic's research subagent prompt sets a budget of under 5 tool calls for simple tasks, about 10 for hard ones and up to 15 for very difficult ones](research/anthropic-research-subagent-prompt-sets-budget-under-tool.md) (high, n.d.)
- [Anthropic's research subagent prompt lists signs of a weak source: aggregators, false authority, nameless sources, vague qualifiers, marketing language, speculation and cherry-picked data](research/anthropic-research-subagent-prompt-lists-signs-weak-source.md) (high, n.d.)
- [Anthropic's research lead prompt sorts queries into depth-first, breadth-first and straightforward, and tells subagents to have distinct, separate tasks](research/anthropic-research-lead-prompt-sorts-queries-depth-first.md) (high, n.d.)
- [Anthropic's multi-agent research system found that agents should search broadly first and then narrow, as expert researchers do](research/anthropic-multi-agent-research-system-found-agents-search.md) (high, 2025-06-13)
- [Anthropic found that every research subagent needs an objective, an output format, guidance on tools and sources, and clear task boundaries](research/anthropic-found-research-subagent-needs-objective-output-format.md) (high, 2025-06-13)
- [Anthropic's early research agents picked search-optimised content farms over authoritative sources until given source-quality heuristics](research/anthropic-early-research-agents-picked-search-optimised-content.md) (high, 2025-06-13)
- [Aider's repo map defaults to a budget of 1,024 tokens and fits the map by binary search until it is within 15 percent of the budget](research/aider-repo-map-defaults-budget-024-tokens-fits.md) (high, n.d.)

## Commons by run

- [261116-172451-lets-runa-lite](commons/261116-172451-lets-runa-lite.md)

## Use it

- Find: `council memory search "<words>"`
- Add by hand: `council memory add --claim "…" --source <url or file> --quote "…"`
- Where this folder lives: `council memory status`
