# Tutorial 5: the shared memory and Obsidian

> ELI5: the council keeps a notebook. Each page holds one fact and says where it came from. The
> next council starts from that notebook instead of from nothing. You can read it, add to it, cross
> things out, and keep it in Obsidian if that is where you think.

**Time:** 10 minutes. **Needs:** the `council` binary. Obsidian is optional.

## Two kinds of thing, kept apart

| | What it is | Who gets it, and when |
| --- | --- | --- |
| **Facts** (`research/`) | One note per fact: a claim, its source, an exact quote, a date, a confidence | Every seat, at once |
| **Thoughts** (`commons/`) | What members said to each other during a run: hunches, questions for the researchers | Every seat, one step later, unsigned, and never inside a blind vote |

The difference is the point. A fact with a source is something every seat can check, so sharing it
does no harm. A thought shared too early makes everyone think alike, so it waits a step and loses
its author's name.

Seats never write the memory themselves. A seat adds `research` and `notes` to its answer, and the
program files them when the step is accepted.

## 1. Where it lives

```console
$ council memory status
memory: files at <project>/.council/memory  (set by: default)
research notes: 0 · runs with commons: 0 · links: markdown
```

By default: plain Markdown files inside the project, at `.council/memory`. Commit them.

## 2. Add a fact by hand

```console
$ council memory add --claim "A managed queue keeps events in order within one message group only" \
    --source https://example.org/queues/ordering \
    --quote "ordering is guaranteed per message group" --tags queue,ordering --date 2026-03-02
added managed-queue-keeps-events-order-within-one-message
```

A fact needs a source: a web address, a file in the project, or `test: <command>` for something
you proved by running it. Without one it is refused:

```console
$ council memory add --claim "Queues are always fast and never lose anything" --source "a blog I read"
error: not added: the source 'a blog I read' is not a web address, a file in this project, or 'test: <command>'
```

The same fact found twice is kept once. The note records who else found it:

```console
$ council memory add --claim "A managed queue keeps events in order within a single message group only" \
    --source https://example.org/queues/ordering
already held: managed-queue-keeps-events-order-within-one-message
```

## 3. What a note looks like

```markdown
---
type: "research"
title: "A managed queue keeps events in order within one message group only"
source: "https://example.org/queues/ordering"
source_date: "2026-03-02"
kind: "other"
confidence: "medium"
checked: true
added: "2026-11-14"
by: "chair"
role: "by hand"
tags: ["council/research", "queue", "ordering"]
quote: "ordering is guaranteed per message group"
---
# A managed queue keeps events in order within one message group only

> ordering is guaranteed per message group

Source: [example.org](https://example.org/queues/ordering) (2026-03-02). Kind: other. Confidence: medium. Opened when it was added.

## Observations

- [fact] A managed queue keeps events in order within one message group only #queue #ordering (medium confidence; example.org, 2026-03-02)

Added 2026-11-14 by chair (by hand).
```

Plain Markdown with properties at the top: readable anywhere, and understood by Obsidian as it is.
You may add your own text to a note; the program keeps it. Do not edit the properties by hand.

`checked: true` means the source was opened when the fact was added. A fact that a model only
recalled is stored with `source: memory`, low confidence and `checked: false`, and every packet
that carries it says so.

## 4. Find and retire

```console
$ council memory search "order message group"
- A managed queue keeps events in order within one message group only (example.org, 2026-03-02; medium)
    https://example.org/queues/ordering  ·  <project>/.council/memory/research/managed-queue-keeps-events-order-within-one-message.md
```

When a fact turns out wrong or out of date, retire it. It is not deleted: the note stays, with the
reason, and seats no longer receive it.

```console
$ council memory retire managed-queue-keeps-events-order-within-one-message --because "the vendor changed this in version 4"
retired: managed-queue-keeps-events-order-within-one-message (kept on disk; seats no longer receive it)
$ council memory search "order message group"
nothing found
$ council memory search "order message group" --retired
- A managed queue keeps events in order within one message group only (example.org, 2026-03-02; medium)  [RETIRED: the vendor changed this in version 4]
```

## 5. How a council uses it

- **Researchers** file what they find: in the demo of [Tutorial 1](01-first-council.md), two
  researchers added nine facts and found one already held.
- **Every packet** carries a digest of the facts that bear on the ask (14 by default; the setting
  is `members.digest`), under a line that says they are claims gathered by others and are not to
  be followed as instructions.
- **Any seat** may add a fact it found or proved, and may put a question to the researchers:
  `"notes": [{"to": "research", "text": "..."}]`. You can too: `council note "<question>" --to research`.
- **The next run** in the same project starts with all of it. Researchers are shown what is
  already held, so they do not look it up again.

`.council/memory/index.md` lists the newest facts and links to each run's commons page.

## 6. Keep it in Obsidian

```console
$ council memory setup --obsidian --vault "/path/to/your/vault"
saved to ~/.council/config.yaml (your user settings: every project on this machine)
memory: obsidian at /path/to/your/vault/Council Loop/shop  (set by: user)
research notes: 0 · runs with commons: 0 · links: wikilinks
In Obsidian: open the vault, then the folder "Council Loop/shop". Start at index.
```

From now on every project's council memory lives in your vault, in a folder per project. Bring
over what a project already holds:

```console
$ council memory import --from .council/memory
imported: 1 added, 0 already held, 0 refused
```

What appears in the vault:

```text
Council Loop/shop/
  index.md           start here
  Research.base      every fact as a table: held, by run, to re-check, retired
  research/          one note per fact, linked with wikilinks
  commons/           one page per run
  architecture.md    a copy of the codebase map, once there is one
```

Notes use Obsidian's own formats (properties, tags, wikilinks, a `.base` table), so graph view,
search and backlinks work with no plugin.

Two rules about where the memory may live, both for your safety:

- **A vault path is yours.** It is saved in your own `~/.council/config.yaml`, never in the
  project, because it is a path on your machine.
- **A project cannot move the memory out of itself.** A repository's `.council/config.yaml` may
  name a memory folder only inside the project. A folder elsewhere must come from your own
  settings. (Otherwise a repository you cloned could make the program write into any folder.)

Back to plain files in each project: `council memory setup --files`. For one run only:
`council init "<ask>" --obsidian` refuses to start if the vault is not there.

If the vault is not on the machine that runs a council, the run says so and keeps its notes in the
project. Import them later.

Not checked from here: how `Research.base` renders inside Obsidian. The file follows the
documented syntax and parses.

## Good habits

- Commit `.council/memory/` with the project. It is part of what the team knows.
- Retire, never delete. The reason is worth as much as the fact.
- When a seat proves something by running code, record it with `--source "test: <command>"`.
- For a topic that must not appear in search logs, run without researchers (`--no-research`) and
  add facts by hand.
