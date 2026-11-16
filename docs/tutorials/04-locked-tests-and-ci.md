# Tutorial 4: locked tests and CI

> ELI5: when a test fails, the easy way out is to change the test. So the tests are written first,
> by someone other than the builder, and then locked like an answer key. The builder can read the
> key but not rewrite it. A hook stops the obvious attempts, and CI stops all of them.

**Time:** 10 minutes. **Needs:** the `council` binary and git.

This is the build half of the Council Loop, and the whole of its Direct tier: one writer, tests
first, tests locked, a verify loop. It works without any council.

## 1. Write the tests first, then lock them

A different agent than the one that will build writes the tests. Then:

```console
$ council lock-tests tests
{"locked": 1, "file": "<project>/.council/locks.json"}
$ cat .council/locks.json
{
 "tests/test_cart.py": "7131a8a84d3fe7e19970a170a4fa13321256ce51ed260ed17e097d2e5f77b4c4"
}
```

The manifest holds one hash per file. Lock anything that is an answer key: tests, migrations that
were already applied, a rubric, golden files. Commit `.council/locks.json`.

```console
$ council verify-locks
{"ok": true, "changed": []}
```

## 2. The hook: what an agent meets

With the plugin installed, or the hook wired by hand (below), Claude Code asks `council hook`
before and after tool calls. An edit of a locked file is denied, with a reason the agent can act on:

```json
{"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny",
  "permissionDecisionReason": "tests/test_cart.py is locked by the Council Loop (tests/answer key). Fix the code, not the test. Ask the requester to unlock if the test is wrong."}}
```

So is a shell line that plainly changes one (`sed -i s/3/4/ tests/test_cart.py`, `rm`, `mv`, a
redirect into the file). Reading the file, running the tests and editing anything else pass
without a word.

A shell can change a file in ways nobody can foresee from the command line: a script, a code
generator, a formatter. So the hook looks again after every shell command. If a locked file no
longer matches, the agent is told at once:

```json
{"decision": "block", "reason": "Locked file(s) no longer match their lock: tests/test_cart.py. Locked files are the answer key: fix the code, not the test. If the command you just ran changed them, undo that change now (for a file git tracks: git checkout -- <file>). If the requester changed them on purpose, do not touch them: ask the requester to lock them again (council lock-tests <paths>)."}
```

The agent cannot lock again by itself either:

```json
{"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny",
  "permissionDecisionReason": "council lock-tests would replace the locks that are in force. Only the requester re-locks, after approving the test change (they set COUNCIL_ALLOW_LOCKED=1 for the session). Ask them."}}
```

Wiring the hook by hand, in `.claude/settings.json` (the plugin does this for you):

```json
{"hooks": {
  "PreToolUse": [{"matcher": "Edit|Write|MultiEdit|NotebookEdit|Bash|PowerShell|Read|Grep|Glob",
    "hooks": [{"type": "command", "command": "council", "args": ["hook"], "timeout": 10}]}],
  "PostToolUse": [{"matcher": "Bash|PowerShell",
    "hooks": [{"type": "command", "command": "council", "args": ["hook"], "timeout": 20}]}]}}
```

The hook starts in about 4 ms with a thousand locked files, so it does not slow a session down.

## 3. CI: the lock that holds

The hook is a seat belt. It exists only in Claude Code, and it can only stop what it can see. The
lock that holds for every tool and every person is a check in CI:

```console
$ council verify-locks --against origin/main
{"ok": false, "changed": ["tests/test_cart.py"], "note": "manifest from origin/main"}
```

`--against` reads the manifest **from the base branch**, not from the pull request. A change that
edits a test and its hash together still fails. And a check that cannot look does not pass:

```console
$ council verify-locks --against origin/main
{"ok": false, "changed": [], "note": "cannot read origin/main: git does not know that ref here. In CI, fetch it first (actions/checkout with fetch-depth: 0)"}
```

A job for GitHub Actions. Copy it into your repository:

```yaml
name: council-locks
on: pull_request
jobs:
  locks:
    # A pull request that changes a locked file on purpose carries the label `council-unlock`,
    # which a person adds after reviewing the test change.
    if: ${{ !contains(github.event.pull_request.labels.*.name, 'council-unlock') }}
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with: {fetch-depth: 0}
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo install --git https://github.com/<your-org>/council-loop --locked
      - run: council verify-locks --against "origin/${{ github.base_ref }}"
```

(When release archives are published, download the binary instead of building it.) Make the job
required in the branch protection of your main branch, and it is no longer up to any agent.

## 4. When a test really is wrong

It happens. The rule is that a person decides, not the agent that wanted the change.

1. The agent says the test is wrong and why. It does not touch it.
2. You review the change to the test. If you agree, you make it, or you let the agent make it in a
   session you started with `COUNCIL_ALLOW_LOCKED=1`.
3. You lock again: `council lock-tests tests`.
4. On the pull request you add the label `council-unlock`, so that the CI job lets this one
   change of the answer key through.

## 5. The Direct tier, end to end

For a small, clear, reversible change there is no council, only this discipline:

```console
$ council init "Rename the seen helper to was_delivered" --stakes low --reversible yes
GATE tier=direct mode=fanout:claude profile=efficient loops<=0 calls~1 tokens~n/a rel_cost=0.66 (chair: stakes=low, reversible)
$ council lock-tests tests          # after a different agent wrote the tests
{"locked": 1, "file": "<project>/.council/locks.json"}
#   ...one writer builds, running the tests, at most five rounds...
$ council verify-locks              # before calling it done
{"ok": true, "changed": []}
$ council ingest                    # closes the run
direct run closed
```

## What this does and does not give you

- It stops an agent from making a failing test pass by editing the test, in the session (the hook)
  and at the gate (CI).
- It does not make the tests good. Tests written by the same agent that builds, or written after
  the code, prove little. That is why a different agent writes them first.
- It does not stop a person with write access from changing a test. It makes the change visible:
  the label and the diff of `locks.json` are in the pull request.
- The hook alone is not the lock. Without the CI job a determined or confused agent can still find
  a way. See [`SECURITY.md`](../../SECURITY.md), "What stays open".
