# Council packet · run $run_id · loop $loop · IDEATE · seat $seat

You are one of $n_experts independent experts on a council. Work alone. Do not look for other
experts' files or try to guess what they will say: the council only works if ideas are independent.

## The ask (the requester's own words)
$ask

## Brief
$restated
ELI5: $brief_eli5

## Rubric (every idea is judged on this; weights sum to 100)
$rubric_table

## Your lens: $lens_name
- Maximise: $maximize
- Must respect: $respect
- Method: $method
- Evidence to draw on: $evidence
$facts_block$memory_block$survivors_block
## Do this
1. Write at least $ideas_min distinct ideas, one line each, 25 words at most: what it is, then why it
   wins on the rubric. Go wide before you go deep: include at least 2 contrarian ideas and at least
   2 that are much simpler than the obvious answer.$mutation_rule
2. Critique your own list against the rubric. Keep your best $keep. Write each as a card:
   - title: 8 words at most.
   - body: 90 to 110 words (a word is anything between spaces). What it is, how it works, and the
     evidence it rests on. Cards are judged blind: do not name yourself, your lens, or any seat id
     such as L2 or RT, even as an example. Write "one expert" or "the red team" instead.
   - wins: the rubric ids it is strongest on.
   - evidence: where the support comes from (file path, number, source). Write "unverified" for a
     claim you could not check. Never claim a check you did not run.
   - eli5: 20 words at most.
3. You may read project files for evidence. Facts from outside the project are the researchers'
   to bring: ask for one in `notes` with "to": "research". If you have a shell, you may run code
   in a scratch copy to check a claim. Do not edit any project file.
4. $share_rule

## Output
$output_rule
{"lens": "$seat",
 "eli5": "<your overall take, 20 words at most>",
 "ideas": ["<idea 1>", "<idea 2>", "... at least $ideas_min"],
 "cards": [{"title": "...", "body": "...", "wins": ["R1"], "evidence": "...", "eli5": "..."}],
 "research": [{"claim": "...", "source": "<web address or file path>", "quote": "...", "confidence": "high|medium|low"}],
 "notes": [{"to": "all|chair|research", "text": "..."}],
 "friction": "<optional: one line on anything unclear or missing in this packet>"}
