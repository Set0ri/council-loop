# Kept cards, loop 1 (6 cards)

Likely overlaps (word overlap; your judgment decides):
- L1#1 ~ L2#1 (29%)

## L1#1: Direct HTTP Dispatch For Zero-Tool Seats
Host subagents incur massive startup overhead by loading system instructions, git status, and environment context on every spawn. In measured runs, rank ballots consumed roughly 64,000 tokens each for a tiny 2,000-token prompt packet, totaling 193,000 tokens across voters. Yet the dispatch logic already classifies vote, rank, control, and elder phases as requiring zero tools. The dispatcher should execute zero-tool seats directly via lightweight HTTP completions whenever provider credentials exist, bypassing full subagent instantiation. This single architectural shift eliminates over 80 percent of ballot and elder token consumption while slashing loop latency without compromising isolation or protocol invariants.
Wins: R1, R2, R4 · Evidence: docs/council-run-2026-11-13-self-review.md (rank vote 193k tokens across 3 ballots, ~64k each); src/run/dispatch.rs (tools_for marks vote, rank, elders, control as 'none')
ELI5: Route zero-tool seats through fast HTTP completions instead of spawning heavy subagents with tens of thousands of startup tokens.

## L1#2: Minimalist Dynamic MCP Toolset Gating
The MCP server currently advertises between 22 and 27 tools on tools/list, outputting 19 to 23 kilobytes of comprehensive JSON schemas. When an AI host initializes or interacts with the server, these exhaustive schemas are injected directly into context, consuming several thousand prompt tokens on every turn. The server should default to a compact core toolset containing only primary workflow operations such as init, next, ingest, status, cut, and note. Specialized administrative tools like blind verification, memory reconfiguration, and lock inspection should remain tucked behind an explicit flag or progressive disclosure. This reduces baseline MCP token consumption without removing any functionality.
Wins: R1, R5, R4 · Evidence: src/mcp.rs (tools_list sends 19 KB to 23 KB across 22 to 27 tool schemas); src/api.rs (registry defines toolset groupings)
ELI5: Default MCP tools list to a compact six-tool core, cutting massive schema bloat on every host prompt.

## L2#1: Direct HTTP Dispatch For Zero-Tool Council Seats
In measured Lite runs, ballot votes consumed 193k tokens across three calls because host subagent wrappers introduced roughly 60k tokens of startup overhead per packet. Protocol validation in src/run/dispatch.rs already flags vote, rank, elder, and control phases as requiring zero tools. When an OpenRouter key or compatible endpoint is present, council dispatch should route zero-tool seats directly through providers::openrouter using the existing ureq client rather than spawning external subagent processes. This eliminates subagent initialization payloads entirely, preserves strict timeout envelopes, and guarantees clean JSON completions while cutting total run spend by over sixty percent without new crate dependencies.
Wins: R1, R2, R4 · Evidence: src/run/dispatch.rs:9-16, src/providers.rs:179-214, docs/council-run-2026-11-13-self-review.md
ELI5: Execute tool-free seats via direct HTTP API calls to eliminate massive subagent startup token overhead.

## L2#2: Static Policy Slug Mapping And Dual Hook Ingestion
Current cross-lab dispatch defaults to stand-ins because models/lock.yaml lacks OpenRouter slugs unless an online refresh was executed, while src/providers.rs only checks raw environment variables. Adding a static slug table in models/policy.yaml gives deterministic cross-lab model resolution offline. Concurrently, src/hook.rs fails on Antigravity IDE events because it only parses snake_case tool_name and tool_input fields from Claude Code instead of camelCase toolCall structures. Extending the hook payload parser to accept both JSON schemas enables seamless IDE integration without proxy wrappers, ensuring that cross-lab seats and automated hooks execute reliably across heterogeneous client environments without touching external crates.
Wins: R2, R3, R5 · Evidence: src/providers.rs:180-182, src/hook.rs:15-35, skills/council-loop/models/policy.yaml
ELI5: Add static provider slugs in policy.yaml and support camelCase IDE hook payloads to ensure reliable multi-client dispatch.

## L3#1: Resilient hook parsing and Windows case-insensitive locking
The pre-tool hook in src/hook.rs expects Claude Code payload keys such as tool_name and tool_input. When executed inside other agent harnesses like Antigravity, payloads provide camelCase fields like toolName or nested toolCall objects, causing the hook to see empty strings and silently permit edits to locked test files. Furthermore, path lookups in locked sets rely on case-sensitive set containment, failing on Windows where filesystems ignore letter casing. By normalizing incoming JSON event keys across snake_case and camelCase variants and applying case-insensitive relative path normalization on Windows platforms, the hook guarantees tamper-proof test locking and seat lane boundaries everywhere.
Wins: R3, R2, R4 · Evidence: src/hook.rs:445-465, hooks/hooks.json, src/safe.rs:224-230
ELI5: Make hooks parse both camelCase and snake_case while matching Windows paths case-insensitively to prevent security bypasses.

## L3#2: Resilient ballot parsing and qualified leak detection
Strict validation in src/run/validate.rs and src/core.rs triggers costly seat respawns over harmless formatting slips. In src/core.rs, label parsing requires exact string boundaries, discarding valid ballots containing markdown bolding or candidate title annotations. Meanwhile, identity leak checks match unqualified two-letter tokens, spuriously flagging standard engineering acronyms and forcing hundred-thousand-token seat retries. Relaxing label extraction to capture leading candidate letters across markdown styles and restricting leak patterns to qualified role references preserves protocol integrity while preventing spurious validation rejections. This drastically cuts wasted startup tokens and unnecessary provider retries across all seats without lowering council evaluation standards.
Wins: R1, R3, R4 · Evidence: src/core.rs:1068-1075, src/core.rs:1242-1260, src/run/validate.rs:200-245, docs/council-run-2026-11-13-self-review.md
ELI5: Allow markdown in ballots and qualify leak checks to prevent expensive validation failures and seat respawns.
