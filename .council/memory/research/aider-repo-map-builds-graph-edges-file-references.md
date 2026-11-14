---
type: "research"
title: "Aider's repo map builds a graph with edges from the file that references a name to the file that defines it, then ranks with PageRank"
source: "https://github.com/Aider-AI/aider/blob/main/aider/repomap.py"
source_date: "n.d."
kind: "repo"
confidence: "high"
checked: true
added: "2026-11-13"
by: "chair"
role: "chair"
model: "Claude Fable 5.2"
tags: ["council/research", "code-map", "aider"]
quote: "G.add_edge(referencer, definer, weight=use_mul * num_refs, ident=ident)"
why: "the ranking method codemap.ranking reimplements"
---
# Aider's repo map builds a graph with edges from the file that references a name to the file that defines it, then ranks with PageRank

> G.add_edge(referencer, definer, weight=use_mul * num_refs, ident=ident)

Source: [github.com](https://github.com/Aider-AI/aider/blob/main/aider/repomap.py) (n.d.). Kind: repo. Confidence: high. Opened when it was added.

Why it matters: the ranking method codemap.ranking reimplements

## Observations

- [fact] Aider's repo map builds a graph with edges from the file that references a name to the file that defines it, then ranks with PageRank #code-map #aider (high confidence; github.com, n.d.)

Added 2026-11-13 by chair (chair, Claude Fable 5.2).
