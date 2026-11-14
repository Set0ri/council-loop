---
type: "research"
title: "Aider multiplies an edge weight by 0.1 for names starting with an underscore and for names defined in more than five files, and takes the square root of the reference count"
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
quote: "if len(defines[ident]) > 5: mul *= 0.1"
why: "damps common names"
---
# Aider multiplies an edge weight by 0.1 for names starting with an underscore and for names defined in more than five files, and takes the square root of the reference count

> if len(defines[ident]) > 5: mul *= 0.1

Source: [github.com](https://github.com/Aider-AI/aider/blob/main/aider/repomap.py) (n.d.). Kind: repo. Confidence: high. Opened when it was added.

Why it matters: damps common names

## Observations

- [fact] Aider multiplies an edge weight by 0.1 for names starting with an underscore and for names defined in more than five files, and takes the square root of the reference count #code-map #aider (high confidence; github.com, n.d.)

## Relations

- same_source [aider-repo-map-builds-graph-edges-file-references](../research/aider-repo-map-builds-graph-edges-file-references.md)
- same_source [aider-multiplies-edge-weight-10-identifiers-chat-mentions](../research/aider-multiplies-edge-weight-10-identifiers-chat-mentions.md)

Added 2026-11-13 by chair (chair, Claude Fable 5.2).
