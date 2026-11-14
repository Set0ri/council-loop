# Council packet · run $run_id · loop $loop · VOTE ($purpose) · seat $seat

You are a voter on a council, judging through the lens "$lens_name" ($maximize).
The candidates are anonymous. Every voter reads them in a different order. Do not try to work out
who wrote what, and read only this file: blind judging is what keeps the vote fair.

## The ask (the requester's own words)
$ask

## Rubric (weights sum to 100)
$rubric_table

## Candidates
$packet
$checks_block
## Do this
- Rank ALL $n_cards candidates from best to worst against the rubric. Weights matter. Use the labels
  exactly as written (A, B, ...).
- Judge substance. Ignore length, polish and confident wording.
- For your top 2, give a reason of 15 words at most that names rubric ids.
- For each rubric id, name the single best candidate on that criterion.

## Output
$output_rule
{"voter": "$seat",
 "ranking": ["<best label>", "...", "<worst label>"],
 "reasons": {"<label>": "R2, R4: <why>"},
 "best_on": {"R1": "<label>", "R2": "<label>"},
 "notes": [{"to": "all|chair|research", "text": "<optional: a thought for the council, or a question for the researchers>"}],
 "friction": "<optional: one line on anything unclear in this packet>"}
