---
type: "research"
title: "Prefix stripping on Windows PathBuf is case-sensitive, permitting hook bypasses on letter-case mismatches."
source: "src/hook.rs"
source_date: "n.d."
kind: "code"
confidence: "high"
checked: true
added: "2026-11-16"
run: "261116-172451-lets-runa-lite"
by: "RT"
role: "red_team"
tags: ["council/research"]
quote: "full.strip_prefix(&root).ok().map(py::posix)"
model: "Claude Fable 5.2 (medium)"
---
# Prefix stripping on Windows PathBuf is case-sensitive, permitting hook bypasses on letter-case mismatches.

> full.strip_prefix(&root).ok().map(py::posix)

Source: `src/hook.rs` (n.d.). Kind: code. Confidence: high. Opened when it was added.

## Observations

- [fact] Prefix stripping on Windows PathBuf is case-sensitive, permitting hook bypasses on letter-case mismatches. (high confidence; src/hook.rs, n.d.)

## Relations

- found_in [261116-172451-lets-runa-lite](../commons/261116-172451-lets-runa-lite.md)

Added 2026-11-16 by RT (red_team, Claude Fable 5.2 (medium)) in run `261116-172451-lets-runa-lite`.
