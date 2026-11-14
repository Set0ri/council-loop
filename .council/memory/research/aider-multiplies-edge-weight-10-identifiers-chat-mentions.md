---
type: "research"
title: "Aider multiplies an edge weight by 10 for identifiers the chat mentions and by 10 for snake, kebab or camel case names of 8 or more characters"
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
quote: "if (is_snake or is_kebab or is_camel) and len(ident) >= 8: mul *= 10"
why: "the weights behind codemap.well_named"
---
# Aider multiplies an edge weight by 10 for identifiers the chat mentions and by 10 for snake, kebab or camel case names of 8 or more characters

> if (is_snake or is_kebab or is_camel) and len(ident) >= 8: mul *= 10

Source: [github.com](https://github.com/Aider-AI/aider/blob/main/aider/repomap.py) (n.d.). Kind: repo. Confidence: high. Opened when it was added.

Why it matters: the weights behind codemap.well_named

## Observations

- [fact] Aider multiplies an edge weight by 10 for identifiers the chat mentions and by 10 for snake, kebab or camel case names of 8 or more characters #code-map #aider (high confidence; github.com, n.d.)

## Relations

- same_source [aider-repo-map-builds-graph-edges-file-references](../research/aider-repo-map-builds-graph-edges-file-references.md)

Added 2026-11-13 by chair (chair, Claude Fable 5.2).
