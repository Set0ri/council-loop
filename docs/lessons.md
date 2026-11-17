# Lessons (itemized, counted, human-merged)

ELI5: A notebook of tips we trust. Every tip has a score. Tips that keep hurting get removed.

Format (append only, via PR):
- [L-YYYYMMDD-n] (helpful: N | harmful: N) One-line lesson. Evidence: <run id or link>

Prune any lesson whose harmful count exceeds its helpful count. Review monthly.

- [L-20261113-1] (helpful: 4 | harmful: 0) Elders independently converged on a front gate plus a blind solo control arm. Size the council to the stakes and keep proving that it beats one strong model. Evidence: docs/council-run-2026-11-13.md
- [L-20261113-2] (helpful: 4 | harmful: 0) Enforce rules below the agent (CI, branch protection, locked tests), not only in prompts or hooks: AGENTS.md tools have no hooks. Evidence: docs/council-run-2026-11-13.md
- [L-20261113-3] (helpful: 1 | harmful: 0) Rank models by Artificial Analysis cost per task, not per-token price. A cheap-per-token model can cost more per task (Gemini 3.8 Flash vs 4 Argon, Nov 2026). Evidence: reference/model-modes.md
- [L-20261113-4] (helpful: 0 | harmful: 0) Before releasing a change to the referee scripts, run a Lite council on the kit itself: three experts, a red team and one elder found 13 real defects that review by one model had missed. Evidence: docs/council-run-2026-11-13-self-review.md
- [L-20261113-5] (helpful: 0 | harmful: 0) Tell the red team and the elders to reproduce claims by running code in a scratch copy: every S1 in this run came from a reproduction, none from reading alone. Evidence: docs/council-run-2026-11-13-self-review.md
- [L-20261113-6] (helpful: 0 | harmful: 0) Count tokens, not calls: a blind vote cost about 64,000 tokens per spawned agent here, nearly all start-up overhead, so route vote, rank and elder seats through a lean no-tool call where one exists. Evidence: docs/council-run-2026-11-13-self-review.md
- [L-20261116-1] (helpful: 0 | harmful: 0) Restrict tool-less HTTP seat dispatch strictly to pure vote and rank ballots: stripping tools from Elders blinds code/test verification probes and was flagged S1 by both Red Team and Elder. Evidence: docs/runs/261116-172451-lets-runa-lite/final.md
- [L-20261116-2] (helpful: 0 | harmful: 0) Probe external API keys (such as OpenRouter HTTP 401 expiry) in council doctor before init assigns crosslab seats so runs fail fast or prompt key renewal instead of silently degrading to stand-ins. Evidence: docs/runs/261116-172451-lets-runa-lite/retro.md
- [L-20261116-3] (helpful: 0 | harmful: 0) Normalize Windows drive-letter casing (c:/ vs C:\) before strip_prefix in memory source validation and hook::inside so valid project file citations and locked-file paths are never rejected or bypassed. Evidence: docs/runs/261116-172451-lets-runa-lite/retro.md
