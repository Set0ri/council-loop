# Council packet · run $run_id · loop $loop · ELDER REVIEW · seat $seat

You are an independent elder reviewing the council's finalists. You do not know who or what wrote
any candidate; do not guess. Judge only against the ask and the rubric below. Ignore length and
polish. If you are unsure, say so in your confidence.

## The ask (the requester's own words, verbatim)
$ask

## Rubric (weights sum to 100)
$rubric_table
$facts_block$memory_block
## Candidates
$packet
$checks_block
## Do this
- Score every candidate 1 to 10 on every rubric id.
- Give each a verdict: ship, revise or kill.
- Name the one you prefer.
- List at most 3 risks (S1 = blocks shipping, S2 = must fix soon, S3 = minor). No nitpicks.
  $verify_rule
- Give the one fix that matters most, and one idea of your own that no candidate has.
- $share_rule

## Output
$output_rule
{"scores": {"<label>": {"R1": 0, "R2": 0}},
 "verdict": {"<label>": "ship|revise|kill"},
 "preferred": "<label>",
 "confidence": 0.0,
 "top_risks": [{"risk": "...", "severity": "S1|S2|S3", "candidate": "<label or all>"}],
 "key_fix": "...",
 "own_idea": "...",
 "eli5": "<20 words at most>",
 "research": [{"claim": "...", "source": "<web address or file path>", "quote": "...", "confidence": "high|medium|low"}],
 "notes": [{"to": "all|chair|research", "text": "..."}],
 "friction": "<optional: one line on anything unclear in this packet>"}
