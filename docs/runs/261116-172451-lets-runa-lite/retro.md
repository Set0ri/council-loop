# Retro: 261116-172451-lets-runa-lite

ELI5: Nine fresh seats beat the one-shot baseline, caught an Elder-blinding S1, and uncovered an expired OpenRouter key.

- What worked: Orthogonal lenses + Red Team code inspection + blind Elder review caught two critical S1 errors in the one-shot control arm (`CTRL` stripped Elder verification tools and hardcoded model names in `policy.yaml`), achieved unanimous `1.0` blind voter agreement (`9/9` seats accepted with `0` retries), and ran voters on `flash_lite` in ~15 seconds.
- What did not: OpenRouter MCP's stored API key had expired (`HTTP 401`), forcing non-Google seats onto fresh-context Gemini 4 stand-ins, and `L3`'s shared facts using lowercase Windows drive prefixes (`c:/Users/...`) were rejected by case-sensitive project-root matching.
- Calls: ideate 4, redteam 1, rank 3, elders 1. Total 9 (the gate estimated 9).
- Loops used: 1 of 1. Rank-vote agreement per loop: 1.0.
- Control arm: the council's answer was preferred by 1 of 1 elder(s)
- Deviations: brief: To save startup tokens (L-20261113-6) and achieve real 3-lab crosslab execution via Antigravity + OpenRouter MCP, L1 runs on OpenAI openai/gpt-6.1-terra-pro, L2 on Anthropic anthropic/claude-haiku-5.5, L3 on Google Gemini 4 Flash subagent (with read-only repo access), CTRL on OpenAI openai/gpt-6.1-sol-pro via an isolated subagent so the chair never sees CTRL before final, and RT/EA on Anthropic anthropic/claude-opus-5.5 via isolated subagents.; loop 1 rank: OpenRouter MCP returned HTTP 401 (API key expired), so L1, L2, CTRL, RT, and EA run as fresh-context Gemini 4 stand-ins rather than external OpenAI/Anthropic endpoints; every stand-in seat is labelled in the final write-up.
- Shared memory (files: <repo>\.council\memory): 7 fact(s) added, 0 already held, 4 refused; 9 note(s), 0 of them questions for the researchers (0 never answered).
- Codebase map: docs/ARCHITECTURE.md (the structure is as mapped on 2026-11-16)
- Friction reported by the seats:
  - L1 (ideate, loop 1): OpenRouter API key expired in local DPAPI vault, demonstrating need for robust fallback to local CLI transports.
  - L2 (ideate, loop 1): OpenRouter API key returned HTTP 401 (expired); seat L2 executed via subagent fresh context.
  - L3 (ideate, loop 1): Packet references offline snapshot facts; live code audit was necessary to verify exact regex and hook behavior.
  - RT (redteam, loop 1): OpenRouter API key expired on test machine (HTTP 401); red team analysis performed via verified code audit stand-in.

## Lessons (0 to 3, one line each; they reach docs/lessons.md only through a PR)
- [L-20261116-1] (helpful: 0 | harmful: 0) Restrict tool-less HTTP seat dispatch strictly to pure vote and rank ballots: stripping tools from Elders blinds code/test verification probes and was flagged S1 by both Red Team and Elder. Evidence: .council/runs/261116-172451-lets-runa-lite/final.md
- [L-20261116-2] (helpful: 0 | harmful: 0) Probe external API keys (such as OpenRouter HTTP 401 expiry) in council doctor before init assigns crosslab seats so runs fail fast or prompt key renewal instead of silently degrading to stand-ins. Evidence: .council/runs/261116-172451-lets-runa-lite/retro.md
- [L-20261116-3] (helpful: 0 | harmful: 0) Normalize Windows drive-letter casing (c:/ vs C:\) before strip_prefix in memory source validation and hook::inside so valid project file citations and locked-file paths are never rejected or bypassed. Evidence: .council/runs/261116-172451-lets-runa-lite/retro.md

## Lessons used in this run (bump helpful or harmful in the same PR)
- L-20261113-1: helpful (+1) — The sealed solo control arm (`CTRL`) made two fatal S1 errors caught blind by the Elder, proving the Lite council earned its cost.
- L-20261113-2: helpful (+1) — Highlighted that Antigravity ignores Claude Code's `hookSpecificOutput` denial JSON, driving `C3`'s dual-envelope + CI lock defense.
- L-20261113-5: helpful (+1) — Code inspection by `RT` and `EA` caught `validate.rs:182`'s non-fatal leak warnings (refuting a false retry claim in `P5`) and `dispatch.rs:96`'s unstripped markdown fences.
- L-20261113-6: helpful (+1) — Running rank voters on lightweight `flash_lite` contexts finished all 3 ballots in ~15s and directly shaped the winning ballot-HTTP architecture (`C2`).
