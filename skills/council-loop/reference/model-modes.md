# Model modes and profiles

ELI5: You pick HOW MANY kinds of AI to use (the mode) and HOW HARD to push them (the profile).
A script reads today's scores and prices and fills in the best model for each job. When better
models come out, you re-run the script, read the diff, and merge.

## Modes

| Mode | Who plays the roles | Use when |
| --- | --- | --- |
| `solo` | The current model plays every role. Only the effort level changes. Elders are fresh contexts | Sensitive code that may not leave one vendor, or no other API keys |
| `fanout:claude` / `fanout:openai` / `fanout:gemini` | A strong chair + cheaper workhorse experts from ONE family. Elders from the same family | You are standardised on one vendor |
| `fanout:openrouter` | Any model reachable through OpenRouter (one key, one bill) | You want breadth without many accounts |
| `fanout:open` | Open-weight models only (self-hosted or via a provider) | Data residency, cost, or no closed models allowed |
| `fanout:mix` | The best-scoring models across all families, picked purely by the numbers | Quality per dollar is all that matters |
| `crosslab` | Experts from mix, guaranteed one seat per lab that clears the floor. Elders are one flagship each from Anthropic, OpenAI and Google | Logan's default for Full councils: decorrelated errors |

## Profiles: one formula for every pick

`score = AA Intelligence Index − k × log2(AA cost per task in USD)`

ELI5: "Each doubling of cost must buy k more points." k = 0 ignores cost. Per role (see `models/policy.yaml`):

| Role | max (k) | balanced (k) | efficient (k) | Other rules |
| --- | --- | --- | --- | --- |
| Chair | 0 | 2 | 6 (and ≥80% of the best quality) | Strongest judgment: it writes the brief and the final |
| Experts | 2 | 6 | 8 | Quality floor vs chair: 0.85 / 0.75 / 0.60. Up to 2 diversity seats within 12 points (crosslab: one seat per lab) |
| Red team | 0 | 2 | 5 | Another family first, then another model, then the chair model in a fresh context |
| Elders | 0 | 2 | 6 | ≥80% of the lab's (or family's) best; crosslab = one per lab |
| Verifier | 2 | 6 | 10 | ≤5 minutes per AA task, quality ≥30 |

Ties within 0.5 points go to the faster model. The k values and floors are starting defaults:
tune them against your golden set.

## Current picks (Artificial Analysis data collected 2026-11-13, Intelligence Index v5.0.1)

Relative cost is in AA cost-per-task units for one Full run, normalised so balanced/crosslab = 1.0.
It compares configurations; it is not a dollar quote.

### Balanced
| Mode | Chair | Experts (7 seats) | Red team | Elders | Rel. cost |
| --- | --- | --- | --- | --- | --- |
| fanout:claude | Claude Fable 5.2 (high) | 5x Claude Fable 5.2 (low) + 1x Claude Opus 5.5 (high) + 1x Claude Sonnet 5.5 (xhigh) | Claude Opus 5.5 (max) | Claude Fable 5.2 (high), Claude Opus 5.5 (max), Claude Sonnet 5.5 (max) | 1.86 |
| fanout:openai | GPT-6.1 Sol (max) | 5x GPT-6.1 Sol (medium) + 1x GPT-6.1 Terra (high) + 1x GPT-6 Astra (high) | GPT-6 Astra (high) | GPT-6.1 Sol (max), GPT-6 Astra (max), GPT-6.1 Terra (max) | 0.44 |
| fanout:gemini | Gemini 4 Argon (high) | 7x Gemini 4 Argon (high) | Gemini 4 Argon (high) | Gemini 4 Argon (high), Gemini 4 Argon (high), Gemini 4 Argon (high) | 1.96 |
| fanout:open | Kimi K3.1 Pro | 7x Kimi K3.1 Pro | Kimi K3.1 Pro | Kimi K3.1 Pro, Kimi K3.1 Pro, Kimi K3.1 Pro | 1.45 |
| fanout:openrouter | Claude Fable 5.2 (high) | 6x GPT-6.1 Sol (high) + 1x Claude Fable 5.2 (low) | GPT-6.1 Sol (max) | Claude Fable 5.2 (high), GPT-6.1 Sol (max), Claude Opus 5.5 (max) | 0.86 |
| fanout:mix | Claude Fable 5.2 (high) | 6x GPT-6.1 Sol (high) + 1x Claude Fable 5.2 (low) | GPT-6.1 Sol (max) | Claude Fable 5.2 (high), GPT-6.1 Sol (max), Claude Opus 5.5 (max) | 0.86 |
| crosslab | Claude Fable 5.2 (high) | 5x GPT-6.1 Sol (high) + 1x Claude Fable 5.2 (low) + 1x Gemini 4 Argon (high) | GPT-6.1 Sol (max) | Claude Fable 5.2 (high), GPT-6.1 Sol (max), Gemini 4 Argon (high) | 1.0 |
| solo | current model @ high | current model @ medium | current model @ high | current model @ high | n/a |

### Max
| Mode | Chair | Experts (7 seats) | Red team | Elders | Rel. cost |
| --- | --- | --- | --- | --- | --- |
| fanout:claude | Claude Fable 5.2 (max) | 6x Claude Fable 5.2 (high) + 1x Claude Opus 5.5 (max) | Claude Opus 5.5 (max) | Claude Fable 5.2 (max), Claude Opus 5.5 (max), Claude Sonnet 5.5 (max) | 4.48 |
| fanout:openai | GPT-6 Astra (max) | 5x GPT-6.1 Sol (max) + 1x GPT-6 Astra (high) + 1x GPT-6.1 Terra (max) | GPT-6.1 Sol (max) | GPT-6 Astra (max), GPT-6.1 Sol (max), GPT-6.1 Terra (max) | 0.8 |
| fanout:gemini | Gemini 4 Argon (high) | 7x Gemini 4 Argon (high) | Gemini 4 Argon (high) | Gemini 4 Argon (high), Gemini 4 Argon (high), Gemini 4 Argon (high) | 1.96 |
| fanout:open | Kimi K3.1 Pro | 7x Kimi K3.1 Pro | Kimi K3.1 Pro | Kimi K3.1 Pro, Kimi K3.1 Pro, Kimi K3.1 Pro | 1.45 |
| fanout:openrouter | Claude Fable 5.2 (max) | 6x Claude Fable 5.2 (high) + 1x GPT-6 Astra (max) | GPT-6 Astra (max) | Claude Fable 5.2 (max), Claude Opus 5.5 (max), GPT-6 Astra (max) | 4.19 |
| fanout:mix | Claude Fable 5.2 (max) | 6x Claude Fable 5.2 (high) + 1x GPT-6 Astra (max) | GPT-6 Astra (max) | Claude Fable 5.2 (max), Claude Opus 5.5 (max), GPT-6 Astra (max) | 4.19 |
| crosslab | Claude Fable 5.2 (max) | 6x Claude Fable 5.2 (high) + 1x GPT-6 Astra (max) | GPT-6 Astra (max) | Claude Fable 5.2 (max), GPT-6 Astra (max), Gemini 4 Argon (high) | 4.14 |
| solo | current model @ max | current model @ high | current model @ high | current model @ max | n/a |

### Efficient
| Mode | Chair | Experts (7 seats) | Red team | Elders | Rel. cost |
| --- | --- | --- | --- | --- | --- |
| fanout:claude | Claude Fable 5.2 (low) | 6x Claude Haiku 5.5 (xhigh) + 1x Claude Fable 5.2 (low) | Claude Opus 5.5 (xhigh) | Claude Fable 5.2 (medium), Claude Opus 5.5 (xhigh), Claude Sonnet 5.5 (max) | 0.66 |
| fanout:openai | GPT-6.1 Sol (medium) | 5x GPT-6.1 Terra (low) + 1x GPT-6.1 Sol (medium) + 1x GPT-6 Luna (max) | GPT-6.1 Terra (high) | GPT-6.1 Sol (medium), GPT-6.1 Terra (max), GPT-6 Astra (high) | 0.11 |
| fanout:gemini | Gemini 4 Argon (high) | 7x Gemini 4 Argon (high) | Gemini 4 Argon (high) | Gemini 4 Argon (high), Gemini 4 Argon (high), Gemini 4 Argon (high) | 1.96 |
| fanout:open | Kimi K3.1 Pro | 6x DeepSeek V4.1 Flash 1017 (max) + 1x Kimi K3.1 Pro | DeepSeek V4.1 Flash 1017 (max) | Kimi K3.1 Pro, Kimi K3.1 Pro, Kimi K3.1 Pro | 0.58 |
| fanout:openrouter | GPT-6.1 Sol (xhigh) | 6x GPT-6.1 Terra (medium) + 1x Claude Haiku 5.5 (xhigh) | Claude Fable 5.2 (medium) | GPT-6.1 Sol (xhigh), GPT-6 Astra (high), Claude Fable 5.2 (medium) | 0.25 |
| fanout:mix | GPT-6.1 Sol (xhigh) | 6x GPT-6.1 Terra (medium) + 1x Claude Haiku 5.5 (xhigh) | Claude Fable 5.2 (medium) | GPT-6.1 Sol (xhigh), GPT-6 Astra (high), Claude Fable 5.2 (medium) | 0.25 |
| crosslab | GPT-6.1 Sol (xhigh) | 5x GPT-6.1 Terra (medium) + 1x Claude Haiku 5.5 (xhigh) + 1x Gemini 4 Argon (high) | Claude Fable 5.2 (medium) | Claude Fable 5.2 (low), GPT-6.1 Sol (medium), Gemini 4 Argon (high) | 0.46 |
| solo | current model @ medium | current model @ low | current model @ medium | current model @ medium | n/a |

What the data says today:
- **Claude Fable 5.2 leads** (AA 65 at max). It is the chair whenever cost allows.
- **GPT-6.1 Sol is the value workhorse**: AA 55 at max for $1.27 per task. In mixed modes it fills
  most expert seats.
- **Gemini's Flash tier costs more per task than Gemini 4 Argon** on AA's agentic tasks ($6.26–7.77
  vs $4.97), so the Gemini-only fan-out has no cheap workhorse today. Argon's intro pricing ends
  2026-11-30; refresh after that.
- **Gemini 4 Carbon** (AA 60) is restricted to Google's Fairwind Program. Set `access` in the policy
  if you have it.
- **Open-weight**: Kimi K3.1 Pro (46) chairs and DeepSeek V4.1 Flash (33, $0.60 per task) is the
  efficient workhorse.

## Keep it current (extensible by design)

```bash
export AA_API_KEY=...                         # free key: artificialanalysis.ai (attribution required)
python3 scripts/refresh_models.py             # pulls live data, writes models/lock.yaml + a dated snapshot
python3 scripts/refresh_models.py --check     # CI/cron: exit 1 when the data is >30 days old
git diff models/lock.yaml                     # review the changes in a PR, like any dependency bump
```

- **New model** → it is picked up automatically if it scores well. No code change needed.
- **New family** → add a filter under `families:` in `policy.yaml`.
- **Can't use a model** → add it to `bans:`, or set `access` for restricted programs.
- **Force a choice** → `pins: {"balanced/crosslab/elders/Google": "Gemini 4 Argon (high)"}`.
- **Different priorities** → change `k`, the floors or `diversity_seats`, re-run, and replay the golden set.
- The snapshot JSON is hand-copied from public AA pages for offline use. Live refreshes use the AA API
  fields `evaluations.artificial_analysis_intelligence_index`,
  `artificial_analysis_intelligence_index_cost.cost_per_task`, `performance_v3.time_per_task_seconds`,
  `licensing.is_open_weights` and `openrouter_api_id`.

Data: [Artificial Analysis](https://artificialanalysis.ai) ([leaderboard](https://artificialanalysis.ai/leaderboards/models/), [open weights](https://artificialanalysis.ai/models/open-source), [API](https://artificialanalysis.ai/data-api/docs)).
