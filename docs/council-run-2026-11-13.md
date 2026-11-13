# Council run 2026-11-13: designing Council Loop v2

ELI5: We used the council process to design the council process. Seven expert viewpoints wrote 91
ideas. Seven whole designs competed. Three AI elders judged them over three rounds: 7 → 5 → 3 → 1.
The winner is what this repo ships.

**Honest deviations from the protocol** (this run invented the protocol, so the tooling didn't exist yet):
- The 7 experts were played by the chair (Claude Opus 5.5) in one context, and the "vote" was the
  chair's rubric scoring. That makes this run closer to `solo` experts.
- The elders were real, separate model instances: Claude Fable (the real Anthropic elder), plus Claude
  Opus and Claude Sonnet in fresh contexts as **labelled stand-ins** for OpenAI and Gemini. Neither
  vendor's models were reachable from the session.
- No control arm was run.

## Stage 0: intent

**Ask (verbatim, condensed):**
- Research the best looped engineering processes for AI. Propose an update to our CLAUDE.md and
  other AI systems.
- Keep Logan's loop: leader → 7 orthogonal experts × ≥13 approaches → self-critique → vote →
  adversarial critique → refine → recombine to the best 7 → rank, ≤3 OODA loops.
- Elders from Gemini, OpenAI and Fable review the top 2 (top 1 in the last loop). Eliminate 7→5→3→1.
- ELI5 everywhere; compressed during iteration, plain language with pictures at the end.

**Added mid-run by Logan:**
- Model modes (solo / fan-out per family incl. OpenRouter, open-source and best mix / cross-lab) ×
  profiles (max, balanced, efficient), chosen from Artificial Analysis data.
- Extensible, because models change constantly.
- The most useful visuals, including the commands and interfaces.
- Research on sizing, context, chunking and memory.
- Ship it as a `.md` skill usable via MCP, submitted as a repo.

ELI5: Make our AI think like a fair, smart team that checks its work, picks the best idea, learns
each time, and can swap in newer AI brains easily.

| Rubric | Weight |
| --- | --- |
| R1 Quality lift (evidence it improves outcomes) | 25 |
| R2 Fidelity to Logan's spec | 20 |
| R3 Verifiability (real checks, not vibes) | 15 |
| R4 Cost and latency proportionality | 10 |
| R5 Robustness to bias and multi-agent failure modes | 15 |
| R6 Implementable today + extensible | 10 |
| R7 Human legibility | 5 |

| Lens | Maximises | Method |
| --- | --- | --- |
| E1 Harness architect | Fit with Claude Code / Codex / Gemini CLI primitives | Systems design |
| E2 Verification scientist | Catching wrong answers | Evals, test-time compute |
| E3 Collective-intelligence scientist | Group judgment quality | Social choice, debate research |
| E4 Search and evolution researcher | Exploration efficiency and diversity | Evolutionary and bandit algorithms |
| E5 Cost and model-routing engineer | Quality per dollar | Artificial Analysis data, budgets |
| E6 Red-team and safety analyst | Fewer failures, less gaming | Pre-mortem, threat modelling |
| E7 Human-factors designer | Adoption and legibility | Plain language, visuals, DX |

## Loop 1

ELI5: Everyone threw in ideas, the best ones were bundled into 7 complete designs, and the elders
liked the careful one but wanted a "small jobs" fast lane.

**91 ideas (7 × 13, ≤12 words each):**
- **E1:**
  short CLAUDE.md + skill · one subagent per role · agent teams · `/council` command · Stop-hook
  verify loop · run ledger on disk · headless `claude -p` fan-out · PAL MCP bridge · CLI bridge
  script · AGENTS.md single source · path-scoped triggers · worktrees for prototypes · model
  registry + adapters
- **E2:**
  rubric before ideas · verifier ladder · evidence-gated critique · chain-of-verification · execute
  spikes · per-stage checklists · held-out golden set · pass^k checks · tests first · confidence +
  "unknown" · position-swapped A/B · calibrate judges vs Logan · ship gate
- **E3:**
  independent ideation · Delphi rounds · Borda voting · Elo tournament · confidence-weighted votes ·
  unique-info-first · minority report · evidence-only vote changes · cross-lab jury · dedicated red
  team · champion debate · equal-length blind cards · chair never authors
- **E4:**
  successive halving · Pareto wildcard · islands · mutation operators · crossover/fusion · MAP-Elites
  niches · cross-run archive · bandit allocation · novelty bonus · backtracking · Self-Discover
  modules · GEPA prompt evolution · wide-early/narrow-late schedule
- **E5:**
  effort tiers · per-stage budgets · cheap ideation / strong judges · parallel calls · early stop ·
  caching · registry of role rules · max/balanced/efficient · family-restricted modes · solo mode ·
  AA cost-per-task · budget kill switch · per-run cost log
- **E6:**
  pre-mortem · MAST checklist · prompt-injection hygiene · no editing own rubric/tests · ≥1 weakness
  per review · severity tags · sandbox · human gate for irreversible steps · citation checks ·
  echo-chamber alarm · copy detection · vendor data policy · audit log
- **E7:**
  ELI5 everywhere · two-speed writing · funnel + OODA diagram · animated explainer · one-page ledger ·
  ADR-style final · loop diffs · show confidence + dissent · glossary · templates · video on request ·
  brief checkpoint · override phrases

**Merged into 11 families:**
1. Architecture
2. Verification
3. Fair judgment
4. Cross-lab elders
5. Evolutionary schedule
6. Proportionality
7. Adversarial safety
8. Legibility
9. Compounding
10. Build loop
11. Model registry

**Fused into 7 candidates (chair rubric score /10):**

| Id | Candidate | Score |
| --- | --- | --- |
| C2 | Verified Council Loop (Logan's loop + verification, fair judgment, wildcard, registry) | 8.35 |
| C7 | Adaptive Router (triage to verify loop / tournament / council) | 7.50 |
| C4 | Lean Verify Loop (tests + Ralph loop; council on demand) | 6.85 |
| C6 | Plan-gated compound engineering | 6.80 |
| C3 | Evolutionary tournament (AlphaEvolve / co-scientist style) | 6.10 |
| C5 | LLM Council only (Karpathy-style) | 5.75 |
| C1 | Faithful council as-is | 5.40 |

**Red team (S1/S2):**
- Cost blow-up on trivial tasks.
- Adoption drag.
- Fake diversity when one model plays every expert.
- Unlabelled stand-ins.
- Vendor data leakage.
- Stale model picks.
- Gaming the loop's own tests.

**Elders on C2 ("K") vs C7 ("Q"), weighted /10:**

| Elder | K | Q | Preferred | Key fix | Own idea |
| --- | --- | --- | --- | --- | --- |
| Fable (real) | 7.65 revise | 6.10 revise | merge | K as engine, Q as a thin front gate; pinned registry via PR | Golden set + single-model baseline arm |
| Opus (stand-in, OpenAI) | 7.40 revise | 5.75 revise | merge | K behind triage; Logan approves the brief; enforce blinding in a script | Blind control arm in the final comparison |
| Sonnet (stand-in, Gemini) | 8.00 ship | 6.55 revise | merge | K + calibrated triage; per-task budget | Held-out replay benchmark vs baseline |

**Cut:**
- C1 eliminated: dominated by C2, which keeps its exact sequence.
- C5 eliminated: no independent ideation or checks. Its anonymised peer ranking was grafted into
  the elder step.
- Survivors: C2, C7, C4, C6, C3.

## Loop 2

ELI5: The survivors borrowed each other's best parts. The careful design won again, and the elders
said "lock the answer key and build it in stages".

**Mutations (top grafts):**
- Gate as a front door with tripwires.
- Logan-approved brief, with the verbatim ask passed to the elders.
- Orchestrator script.
- Control arm.
- Golden set.
- Registry refresh by PR.
- Tests-first Direct tier.

**Chair rubric scores:**

| Candidate | Score |
| --- | --- |
| K2 Verified Council v2 | 8.75 |
| Q2 Router-first | 7.10 |
| V2 Lean Verify + mini-council | 7.10 |
| P2 Plan-gated | 7.00 |
| T2 Tournament | 6.80 (unique best on R3 → wildcard) |

V2 went to the elders over Q2 on the tie-break (the most different from the leader; Q2's gate was
already merged into K2).

| Elder | K2 | V2 | Preferred | Insisted on | Own idea |
| --- | --- | --- | --- | --- | --- |
| Fable | 8.25 ship | 6.30 revise | K2 | Ship in stages; downgrades only by PR after ≥10 runs | Outcome ledger (2–4 week follow-up) |
| Opus (stand-in) | 7.80 revise | 6.55 kill | K2 | Tests written first and hash-locked; tripwires only escalate | Prediction ledger with falsifiable metrics |
| Sonnet (stand-in) | 8.25 ship | 6.75 revise | merge | Independent test author or mutation checks; staged build | Disagreement → ask Logan one question |

**Cut:**
- Q2 eliminated (absorbed as K2's gate).
- P2 eliminated (human gates and lessons absorbed).
- Survivors: K2, V2, T2 (wildcard).

## Loop 3

ELI5: The three survivors merged into one design. All three elders said "ship". Each asked for one
thing to be made safer or simpler first.

**Final candidate F = K2 + V2** (the Direct tier and the build step) **+ T2** (Tournament sub-mode),
**plus:**
- Sizing, context and memory rules.
- The model registry (modes × profiles).
- Repo packaging.

| Elder | F | Verdict | Must change before ship | Cut if overbuilt | Own idea |
| --- | --- | --- | --- | --- | --- |
| Fable | 8.35 | ship (0.80) | One enforcement path: the CLI is the source of truth; drop MCP from v1 | MCP, tournament, extra fan-out variants | Dry-run mode |
| Opus (stand-in) | 8.15 | ship (0.80) | Enforce below the agent: CI / branch protection; no prod credentials | Context rulebook detail, MCP prompts, tournament | Monthly fire drill |
| Sonnet (stand-in) | 8.30 | ship (0.75) | Ship Stage 1 alone, then a go/no-go blind A/B on ≥20 tasks | MCP, OpenRouter/mix variants, tournament | "Process off" canary, 1 task in 10 |

**How the final answered them:**
- **One path.** `council.py` is the single source of truth. The MCP server, the hook and CI are thin
  wrappers over it. ✔
- **MCP kept.** Logan explicitly asked for it. Since it is a thin wrapper, keeping it costs little;
  this dissent is recorded.
- **Enforced below the agent.** CI runs `verify-locks`, with a `council-unlock` label for exceptions,
  and agents get no publishing credentials. ✔
- **Staged rollout.** Stage 1 first, with a pre-registered go/no-go. ✔
- **Dry run.** `council.py plan` and the "dry run" phrase. ✔
- **Fire drill and process-off canary:** in the rollout. ✔
- **Fan-out variants kept** (Logan asked for them). They are computed rows in `lock.yaml`, with no
  extra code.

**Winner:** Council Loop v2. Elder average 8.27/10, three of three "ship".
