"""The run loop, driven end to end with fake seats (tests/sim.py). Tests named after a finding are
regression tests for defects that the kit's own council found (docs/council-run-2026-11-13-self-review.md)."""
import json
from pathlib import Path

import pytest
import yaml

import council
import run as runner
import sim

TESTS = Path(__file__).resolve().parent


def current() -> tuple[dict, Path]:
    return runner.load(None)


def start(tier="lite", mode="fanout:claude", ask="Pick a queue for billing events", loops=None, profile="balanced",
          research=None, mapper=None, through_research=True, obsidian=False, paths=(), **settings):
    runner.cmd_init(ask, tier, mode, profile, loops, list(paths), None, None, None, research, mapper, obsidian, **settings)
    run, d = current()
    write_brief(d, run)
    runner.cmd_ingest()
    if through_research:
        do_research(d)
    return current()


def do_research(d: Path, k: int = 1, mode="ok"):
    """Play the researchers (and the mapper, when it is due) if the run is at that step."""
    run, _ = current()
    if run["phase"] != "research":
        return
    if any(v["via"].startswith("cli:") for v in run["seats"].values()):
        runner.cmd_dispatch()                                # seats behind a command-line tool answer through it
    else:
        sim.act_all(d, f"loop-{k}/research/*.prompt.md", mode)
    runner.cmd_ingest()


def write_brief(d: Path, run: dict):
    brief = {"restated": "Pick the queue technology for the billing pipeline.", "eli5": "choose the safest mail system",
             "rubric": [{"id": f"R{i + 1}", "name": n, "weight": w, "check": "judgment"}
                        for i, (n, w) in enumerate([("reliability", 40), ("cost", 20), ("simplicity", 20),
                                                    ("speed to ship", 10), ("operability", 10)])],
             "evidence_plan": ["load test"],
             "lenses": [{"id": s, "name": f"lens {s[1:]}", "maximize": "one thing", "respect": "a limit",
                         "method": "first principles", "evidence": "facts"} for s in runner.seats_of(run, "expert")],
             "facts": "Peak is 4,000 events per second.", "lessons_applied": ["L-20261113-1"], "approved_by": "requester"}
    (d / "brief.yaml").write_text(yaml.safe_dump(brief), encoding="utf-8")


def fill_fuse(d: Path, k: int):
    f = d / f"loop-{k}" / "candidates.json"
    obj = json.loads(f.read_text(encoding="utf-8"))
    for r in obj["responses"]:
        r.update(action="fixed", note="added idempotency keys to every consumer")
    f.write_text(json.dumps(obj), encoding="utf-8")


def approve_pool(d: Path, k: int):
    f = d / f"loop-{k}" / "pool.json"
    obj = json.loads(f.read_text(encoding="utf-8"))
    obj["reviewed"] = True
    f.write_text(json.dumps(obj), encoding="utf-8")


def to_fuse(d: Path, k: int, ideate_mode="ok"):
    sim.act_all(d, f"loop-{k}/ideate/*.prompt.md", ideate_mode)
    if k == 1:
        sim.act(d / "control" / "CTRL.prompt.md")
    runner.cmd_ingest()                                      # -> merge
    approve_pool(d, k)
    runner.cmd_ingest()                                      # -> vote or redteam
    if current()[0]["phase"] == "vote":
        sim.act_all(d, f"loop-{k}/vote/*.prompt.md")
        runner.cmd_ingest()
    sim.act_all(d, f"loop-{k}/redteam/*.prompt.md")
    runner.cmd_ingest()                                      # -> fuse


def one_loop(d: Path, k: int, elder_mode="ok", target=None, ideate_mode="ok", vote_mode="ok"):
    to_fuse(d, k, ideate_mode)
    fill_fuse(d, k)
    runner.cmd_ingest()                                      # -> rank
    sim.act_all(d, f"loop-{k}/rank/*.prompt.md", vote_mode)
    runner.cmd_ingest()                                      # -> elders
    sim.act_all(d, f"loop-{k}/elders/*.prompt.md", elder_mode, target)
    runner.cmd_ingest()                                      # -> cut


def finish(d: Path):
    f = d / "final.md"
    f.write_text(f.read_text(encoding="utf-8").replace("TODO(chair)", "done") + "\nDecided unattended where noted. stand-in seats are labelled.\n", encoding="utf-8")
    runner.cmd_ingest()                                      # -> retro
    r = d / "retro.md"
    text = r.read_text(encoding="utf-8").replace("TODO(chair)", "done").replace(
        "they reach docs/lessons.md only through a PR)",
        "they reach docs/lessons.md only through a PR)\n- [L-20261114-1] (helpful: 0 | harmful: 0) Lock the tests first. Evidence: this run")
    r.write_text(text, encoding="utf-8")
    runner.cmd_ingest()                                      # -> done


# ----------------------------------------------------------------------------- whole runs
def test_lite_run_end_to_end(proj, capsys):
    assert runner.cmd_init("Pick the queue technology for the billing pipeline", "lite", "fanout:claude",
                           "balanced", None, [], None, None, None) == 0
    run, d = current()
    assert run["phase"] == "brief" and run["schedule"] == [3, 1] and len(runner.seats_of(run, "expert")) == 3
    out = capsys.readouterr().out
    assert "L-20261113-1" in out and "tokens~" in out          # lessons and a token estimate are shown at the start
    write_brief(d, run)
    runner.cmd_ingest()
    assert current()[0]["phase"] == "research"                # the researchers go first; no code here, so no mapper
    do_research(d)
    one_loop(d, 1)
    run, _ = current()
    assert run["phase"] == "cut" and "vote" not in run["loops"][0]   # lite skips the shortlist vote
    runner.cmd_cut(None, "three experts suggested queues; one clear winner")
    run, _ = current()
    assert run["phase"] == "final" and run["winner"] and run["control_result"] in ("won", "lost", "tie")
    finish(d)
    run, _ = current()
    assert run["phase"] == "done" and council.check_run(run)["ok"]
    assert (d / "loop-1" / "ledger.md").read_text(encoding="utf-8").startswith("# Loop 1 ledger")
    assert (d / "lessons.proposed.md").read_text(encoding="utf-8").startswith("- [L-20261114-1]")
    assert any("facts section was thin" in x for x in run["friction"])       # seat friction reaches the retro
    assert len(run["friction"]) == len(set(run["friction"]))                 # ...once each
    assert "just do the obvious thing" not in (d / "run.json").read_text(encoding="utf-8")   # the control answer never enters run.json
    assert (d / "unblinded.json").exists() and runner.cmd_audit() == 0       # unblinded at the end; the record re-counts


def test_full_run_three_loops_follows_schedule(proj):
    run, d = start("auto", ask="Write an RFC for our new auth architecture", profile="efficient")
    assert run["tier"] == "full" and run["schedule"] == [7, 5, 3, 1] and "auth/security" in run["tripwires"]
    for k, expect in ((1, 5), (2, 3), (3, 1)):
        one_loop(d, k)
        runner.cmd_cut(None, f"loop {k} narrowed the field")
        run, _ = current()
        L = run["loops"][k - 1]
        assert len(L["survivors"]) == expect
        assert sum(len(v) for v in L["ideas"].values()) >= 7 * 13
        assert len(L["candidates"]) == [7, 5, 3][k - 1]
    assert run["phase"] == "final" and "vote" in run["loops"][0]
    assert sum(run["calls"].values()) == run["calls_estimate"] == 78       # 76 council calls + the two researchers
    assert "Survivors of the last loop" in (d / "loop-2" / "ideate" / "L1.prompt.md").read_text(encoding="utf-8")
    assert "replay a golden set first" in (d / "loop-2" / "ideate" / "L1.prompt.md").read_text(encoding="utf-8")   # elder ideas feed forward
    finish(d)
    run, _ = current()
    assert run["phase"] == "done" and council.check_run(run)["ok"] and runner.cmd_audit() == 0


# ----------------------------------------------------------------------------- retries, restarts, drops
def test_rejected_output_is_moved_aside_and_counted_once(proj):
    """Elder S2: re-running ingest used to count the same rejected output again and again."""
    run, d = start()
    sim.act(d / "loop-1/ideate/L1.prompt.md", "few")          # only 2 ideas
    sim.act(d / "loop-1/ideate/L2.prompt.md", "fenced")       # JSON inside a code fence: still accepted
    sim.act(d / "control/CTRL.prompt.md")
    with pytest.raises(runner.RunError) as err:
        runner.cmd_ingest()
    msg = str(err.value)
    assert "L1: REJECTED (output 1): 2 ideas, need at least 5" in msg and "L3: no output yet" in msg
    assert "L1.retry.md" in msg and "[retry]" in msg           # the spawn line points at the retry prompt
    assert "rejected" in (d / "loop-1/ideate/L1.retry.md").read_text(encoding="utf-8")
    assert not (d / "loop-1/ideate/L1.out.json").exists() and (d / "loop-1/ideate/L1.rejected-1.json").exists()
    for _ in range(3):                                         # waiting is not failing
        with pytest.raises(runner.RunError) as err:
            runner.cmd_ingest()
    assert "L1: its retry has not come back yet" in str(err.value)
    run, _ = current()
    assert run["phase"] == "ideate" and run["attempts"] == {"1/ideate/L1": 1}
    sim.act(d / "loop-1/ideate/L1.prompt.md")
    sim.act(d / "loop-1/ideate/L3.prompt.md")
    runner.cmd_ingest()
    run, _ = current()
    assert run["phase"] == "merge" and run["calls"]["ideate"] == 5       # 4 accepted + 1 rejected


def test_long_idea_lines_warn_and_never_cost_a_respawn(proj, capsys):
    run, d = start()
    sim.act_all(d, "loop-1/ideate/*.prompt.md", "long")
    sim.act(d / "control/CTRL.prompt.md")
    runner.cmd_ingest()
    assert current()[0]["phase"] == "merge" and "are over 25 words" in capsys.readouterr().out


def test_next_after_a_restart_lists_only_the_seats_still_owed(proj, capsys):
    run, d = start()
    sim.act(d / "loop-1/ideate/L1.prompt.md")
    sim.act(d / "loop-1/ideate/L2.prompt.md")
    before = (d / "loop-1/ideate/L1.out.json").read_text(encoding="utf-8")
    capsys.readouterr()
    runner.cmd_next()                                          # a chair that lost its context runs this again
    out = capsys.readouterr().out
    assert "2 of 4 seat(s) are already in (L1, L2)" in out and "SPAWN 2 fresh-context" in out
    assert "L3.prompt.md" in out and "CTRL.prompt.md" in out and "L1.prompt.md" not in out
    assert (d / "loop-1/ideate/L1.out.json").read_text(encoding="utf-8") == before
    runner.cmd_status()
    assert "2 of 4 seat(s) in; still owed: L3 (missing), CTRL (missing)" in capsys.readouterr().out


def test_drop_names_the_seat_and_cannot_take_the_control_arm_with_it(proj):
    """Red-team S2: bare --force dropped every pending seat, including the control arm."""
    run, d = start()
    sim.act(d / "loop-1/ideate/L1.prompt.md")
    with pytest.raises(runner.RunError) as err:                # 1 valid expert < quorum of 2
        runner.cmd_ingest(drop="L2,L3,CTRL", reason="they timed out twice")
    assert "this step needs 2" in str(err.value)
    sim.act(d / "loop-1/ideate/L2.prompt.md")
    with pytest.raises(runner.RunError) as err:                # CTRL is still owed and was not named
        runner.cmd_ingest(drop="L3", reason="L3 timed out twice")
    assert "CTRL: no output yet" in str(err.value) and "control" not in current()[0]
    with pytest.raises(runner.RunError) as err:
        runner.cmd_ingest(drop="L1,L3", reason="trying to drop a good seat")
    assert "L1 returned a valid output" in str(err.value)
    with pytest.raises(runner.RunError) as err:
        runner.cmd_ingest(drop="L3")
    assert "--drop needs --reason" in str(err.value)
    sim.act(d / "control/CTRL.prompt.md")
    runner.cmd_ingest(drop="L3", reason="L3 timed out twice")
    run, _ = current()
    assert run["phase"] == "merge" and run["control"]["done"] and len(run["control"]["sha256"]) == 64
    assert any("seat L3 was left out" in x and "timed out twice" in x for x in run["deviations"])


def test_first_rejection_cannot_be_dropped_and_control_drop_is_explicit(proj):
    run, d = start()
    sim.act_all(d, "loop-1/ideate/L[12].prompt.md")
    sim.act(d / "loop-1/ideate/L3.prompt.md", "few")
    with pytest.raises(runner.RunError) as err:
        runner.cmd_ingest(drop="L3,CTRL", reason="no time for a retry")
    assert "L3 was rejected for the first time just now: retry it once first" in str(err.value)
    runner.cmd_ingest(drop="L3,CTRL", reason="L3 failed and never came back; the control arm host is down")
    run, _ = current()
    assert run["phase"] == "merge" and run["control"]["done"] is False and "host is down" in run["control"]["dropped"]


# ----------------------------------------------------------------------------- blinding and provenance
def test_authors_come_from_provenance_not_from_typing(proj):
    """Card L2#1: the chair typed the authors by hand, so a typo let experts rank their own work."""
    run, d = start()
    sim.act_all(d, "loop-1/ideate/*.prompt.md")
    sim.act(d / "control/CTRL.prompt.md")
    runner.cmd_ingest()
    f = d / "loop-1/pool.json"
    obj = json.loads(f.read_text(encoding="utf-8"))
    assert all("authors" not in c for c in obj["pool"])        # nothing to type
    merged = obj["pool"][0]
    merged["from"] = ["L1#1", "L2#1"]                          # merge two experts' cards...
    merged["authors"] = ["L3"]                                 # ...and try to mislabel the result
    obj["pool"] = [c for c in obj["pool"] if c["from"] != ["L2#1"] or c is merged]
    obj["reviewed"] = True
    f.write_text(json.dumps(obj), encoding="utf-8")
    runner.cmd_ingest()
    run, _ = current()
    pool = {c["id"]: c for c in run["loops"][0]["pool"]}
    assert pool[merged["id"]]["authors"] == ["L1", "L2"]
    sim.act_all(d, "loop-1/redteam/*.prompt.md")
    runner.cmd_ingest()
    c = d / "loop-1/candidates.json"
    obj = json.loads(c.read_text(encoding="utf-8"))
    assert all("authors" not in x for x in obj["candidates"])
    obj["candidates"][1]["parents"] = [merged["id"], obj["candidates"][1]["parents"][0]]
    c.write_text(json.dumps(obj), encoding="utf-8")
    fill_fuse(d, 1)
    runner.cmd_ingest()
    cands = current()[0]["loops"][0]["candidates"]
    assert set(cands[1]["authors"]) >= {"L1", "L2"}            # the union of its parents' authors
    assert current()[0]["loops"][0]["not_carried"]             # what the chair left out is on the record


def test_merge_may_not_lose_a_kept_card_or_invent_a_source(proj):
    run, d = start()
    sim.act_all(d, "loop-1/ideate/*.prompt.md")
    sim.act(d / "control/CTRL.prompt.md")
    runner.cmd_ingest()
    f = d / "loop-1/pool.json"
    obj = json.loads(f.read_text(encoding="utf-8"))
    obj["reviewed"] = True
    full = json.dumps(obj)
    obj["pool"] = obj["pool"][:-1]
    f.write_text(json.dumps(obj), encoding="utf-8")
    with pytest.raises(runner.RunError) as err:
        runner.cmd_ingest()
    assert "L3#2 are in no pool card" in str(err.value)
    obj = json.loads(full)
    obj["pool"][0]["from"] = ["L9#1"]
    f.write_text(json.dumps(obj), encoding="utf-8")
    with pytest.raises(runner.RunError) as err:
        runner.cmd_ingest()
    assert '"from" must list the kept card(s)' in str(err.value)


def test_every_voter_reads_the_cards_in_its_own_order(proj):
    """Card L2#2 and red-team S2: one shared order gave the first card the same boost on every ballot."""
    run, d = start()
    to_fuse(d, 1)
    fill_fuse(d, 1)
    runner.cmd_ingest()
    first = {s: sim.labels((d / f"loop-1/rank/{s}.prompt.md").read_text(encoding="utf-8"))[0] for s in ("L1", "L2", "L3")}
    assert sorted(first.values()) == ["A", "B", "C"]
    assert "Machine checks" in (d / "loop-1/rank/L1.prompt.md").read_text(encoding="utf-8")


def test_leaky_control_card_is_rejected_while_a_retry_is_cheap(proj):
    """Control arm's own finding: the control card was blind-checked only at the elder step."""
    run, d = start()
    sim.act_all(d, "loop-1/ideate/*.prompt.md")
    sim.act(d / "control/CTRL.prompt.md", "leak")
    with pytest.raises(runner.RunError) as err:
        runner.cmd_ingest()
    assert "CTRL: REJECTED" in str(err.value) and "identity leak" in str(err.value)
    assert "identity leak" in (d / "control/CTRL.retry.md").read_text(encoding="utf-8")


def test_sealed_control_arm_cannot_vanish_silently(proj, capsys):
    """Elder S2: a lost control card was dropped silently and the record still said the council won."""
    run, d = start()
    to_fuse(d, 1)
    fill_fuse(d, 1)
    runner.cmd_ingest()
    sim.act_all(d, "loop-1/rank/*.prompt.md")
    (d / "control/CTRL.out.json").write_text(json.dumps({"title": "swapped", "body": sim.body("x"), "eli5": "hm"}), encoding="utf-8")
    with pytest.raises(runner.RunError) as err:
        runner.cmd_ingest()                                    # ingest_rank advances, then next_elders refuses
    assert "the control arm is sealed" in str(err.value) and "--without-control" in str(err.value)
    assert runner.cmd_audit() == 1
    runner.cmd_next(without_control="the file was overwritten by a bad sync")
    sim.act_all(d, "loop-1/elders/*.prompt.md")
    runner.cmd_ingest()
    runner.cmd_cut(None, "went on without the baseline")
    run, _ = current()
    assert run["control_result"] == "not run" and "bad sync" in run["control_line"]
    assert any("control arm was lost" in x for x in run["deviations"])


# ----------------------------------------------------------------------------- elders and the cut
def test_elder_scores_may_have_gaps_without_crashing(proj, capsys):
    """Red-team S2: an elder that skipped one card's scores crashed ingest with a KeyError."""
    run, d = start()
    one_loop(d, 1, elder_mode="noscores")
    assert current()[0]["phase"] == "cut" and "scores for" in capsys.readouterr().out


def test_elder_and_red_team_labels_are_read_tolerantly(proj):
    """A retry is a whole new agent, so "Candidate A" for "A" must not cost one."""
    run, d = start()
    sim.act_all(d, "loop-1/ideate/*.prompt.md")
    sim.act(d / "control/CTRL.prompt.md")
    runner.cmd_ingest()
    approve_pool(d, 1)
    runner.cmd_ingest()
    f = sim.act(d / "loop-1/redteam/RT.prompt.md")
    obj = json.loads(f.read_text(encoding="utf-8"))
    obj["findings"][0]["candidate"] = "Candidate " + obj["findings"][0]["candidate"].lower()
    obj["findings"][1]["candidate"] = "ALL"
    f.write_text(json.dumps(obj), encoding="utf-8")
    runner.cmd_ingest()
    fill_fuse(d, 1)
    runner.cmd_ingest()
    sim.act_all(d, "loop-1/rank/*.prompt.md")
    runner.cmd_ingest()
    f = sim.act(d / "loop-1/elders/EA.prompt.md")
    obj = json.loads(f.read_text(encoding="utf-8"))
    obj = {**obj, "verdict": {f"Candidate {k}": v.upper() for k, v in obj["verdict"].items()},
           "scores": {f"candidate {k.lower()}": v for k, v in obj["scores"].items()}, "preferred": f"Candidate {obj['preferred']}."}
    f.write_text(json.dumps(obj), encoding="utf-8")
    runner.cmd_ingest()
    run, _ = current()
    rv = run["loops"][0]["elder_reviews"][0]
    assert run["phase"] == "cut" and set(rv["verdict"].values()) == {"ship"} and all(rv["scores"].values())
    assert run["loops"][0]["red_team"][1]["candidate_id"] == "all" and not run.get("attempts")


def test_winner_the_elders_never_reviewed_is_a_judgment_point(proj):
    """Idea L2-3: a killed leader handed the win to a card no elder had seen."""
    run, d = start("full", ask="Write an RFC for our new auth architecture", loops=1)
    assert run["schedule"] == [7, 1]
    to_fuse(d, 1)
    fill_fuse(d, 1)
    runner.cmd_ingest()
    sim.act_all(d, "loop-1/rank/*.prompt.md")
    runner.cmd_ingest()                                        # -> elders: top 1 plus the control arm
    run, _ = current()
    leader = run["loops"][0]["rank"]["order"][0]
    mapping = json.loads((proj / "private" / f"{run['id']}.json").read_text(encoding="utf-8"))["1/elders"]
    target = next(l for l, v in mapping.items() if v["id"] == leader)
    sim.act_all(d, "loop-1/elders/*.prompt.md", "kill", target)
    runner.cmd_ingest()
    with pytest.raises(runner.RunError) as err:
        runner.cmd_cut(None, "elders killed the leader")
    assert "never reviewed the leader" in str(err.value)
    runner.cmd_cut(None, "elders killed the leader", unattended="take the runner-up but do not ship before the requester reads it")
    run, _ = current()
    assert run["winner"] != leader and any("never reviewed the leader" in x for x in run["deviations"])
    assert run["control_result"] == "lost"                     # every elder preferred the baseline


def test_vote_may_not_cut_what_it_could_not_judge(proj):
    """Elder S1: a head-to-head nobody could judge counted as even, and the card was cut unseen."""
    run, d = start()
    to_fuse(d, 1)
    fill_fuse(d, 1)
    runner.cmd_ingest()
    sim.act(d / "loop-1/rank/L1.prompt.md")
    sim.act(d / "loop-1/rank/L2.prompt.md")
    runner.cmd_ingest(drop="L3", reason="the third voter never came back")
    run, _ = current()
    L = run["loops"][0]
    assert L["rank"]["thin"], "two sole authors and no third voter: some head-to-heads had no judge"
    assert L["protected_elders"] and len(L["reviewed"]) == 3   # all three go to the elder, not just the top two
    assert any("the vote could not judge it" in x for x in run["deviations"])


def test_split_elders_need_an_answer_or_an_unattended_choice(proj):
    run, d = start("full", ask="Write an RFC for our new auth architecture")
    to_fuse(d, 1)
    fill_fuse(d, 1)
    runner.cmd_ingest()
    sim.act_all(d, "loop-1/rank/*.prompt.md")
    runner.cmd_ingest()
    sim.act_all(d, "loop-1/elders/*.prompt.md")
    f = d / "loop-1/elders/EB.out.json"                        # one elder prefers another candidate
    obj = json.loads(f.read_text(encoding="utf-8"))
    obj["preferred"] = sorted(obj["verdict"])[1]
    f.write_text(json.dumps(obj), encoding="utf-8")
    runner.cmd_ingest()
    with pytest.raises(runner.RunError) as err:
        runner.cmd_cut(None, "the elders split")
    assert "judgment point (the elders split)" in str(err.value)
    runner.cmd_cut(None, "the elders split", asked="Speed or safety first? -> Safety")
    assert current()[0]["loops"][0]["asked"].endswith("Safety")


def test_brief_validation_messages(proj):
    runner.cmd_init("Pick a queue for billing events", "lite", "fanout:claude", "balanced", None, [], None, None, None)
    run, d = current()
    (d / "brief.yaml").write_text(yaml.safe_dump({"restated": "x", "eli5": "y", "rubric": [{"id": "R1", "name": "a", "weight": 50}],
                                                 "lenses": [], "lessons_applied": [], "approved_by": ""}), encoding="utf-8")
    with pytest.raises(runner.RunError) as err:
        runner.cmd_ingest()
    msg = str(err.value)
    assert "rubric needs 3 to 7 criteria" in msg and "weights must sum to 100" in msg and "approved_by" in msg


def test_a_person_approves_the_brief_and_the_chair_cannot(proj):
    runner.cmd_init("Pick a queue for billing events", "lite", "fanout:claude", "balanced", None, [], None, None, None,
                    research=0, mapper="never")
    run, d = current()
    write_brief(d, run)
    brief = yaml.safe_load((d / "brief.yaml").read_text(encoding="utf-8"))
    for who in ("chair", "Claude", "yes", "  ", None, 7):            # none of these is a person who read the brief
        (d / "brief.yaml").write_text(yaml.safe_dump({**brief, "approved_by": who}), encoding="utf-8")
        with pytest.raises(runner.RunError) as err:
            runner.cmd_ingest()
        assert "approved_by must name the person" in str(err.value) and current()[0]["phase"] == "brief"
    (d / "brief.yaml").write_text(yaml.safe_dump({**brief, "approved_by": " Ada Lovelace "}), encoding="utf-8")
    runner.cmd_ingest()                                              # any person's name will do, whoever asked
    run = current()[0]
    assert run["brief"]["approved_by"] == "Ada Lovelace" and run["phase"] == "ideate" and not run["deviations"]


def test_an_unattended_full_brief_is_written_down_as_a_deviation(proj):
    runner.cmd_init("Write an RFC for our new auth architecture", "full", "fanout:claude", "balanced", None, [], None, None,
                    None, research=0, mapper="never")
    run, d = current()
    write_brief(d, run)
    brief = yaml.safe_load((d / "brief.yaml").read_text(encoding="utf-8"))
    (d / "brief.yaml").write_text(yaml.safe_dump({**brief, "approved_by": "Unattended"}), encoding="utf-8")
    runner.cmd_ingest()
    run = current()[0]
    assert run["brief"]["approved_by"] == "unattended"
    assert any("not approved by a person" in x for x in run["deviations"])


def test_seats_become_labelled_stand_ins_when_a_lab_is_unreachable(proj, capsys):
    runner.cmd_init("Pick a queue for billing events", "lite", "crosslab", "balanced", None, [], None, None, None)
    run, _ = current()
    standins = [s for s, v in run["seats"].items() if v.get("stand_in_for")]
    assert standins, "codex/gemini are not installed in CI, so OpenAI/Google seats must be stand-ins"
    assert "STAND-IN" in capsys.readouterr().out


def test_direct_tier_is_a_checklist(proj, capsys):
    runner.cmd_init("rename a variable in the parser", "auto", None, "balanced", None, [], "low", "yes", None)
    run, _ = current()
    assert run["tier"] == "direct" and run["phase"] == "direct"
    assert "lock-tests" in capsys.readouterr().out
    runner.cmd_ingest()
    assert current()[0]["phase"] == "done"


def test_deviation_command_reaches_the_record(proj):
    run, d = start()
    runner.cmd_deviation(None, "ran the red team on a cheaper model because the quota ran out")
    assert "cheaper model" in current()[0]["deviations"][-1]


# ----------------------------------------------------------------------------- seats behind command-line tools
def test_cli_seats_dispatch_and_a_rejected_one_can_really_be_retried(fake_clis, monkeypatch, capsys):
    """Cards L1#1 and L3#2 (found independently): the printed retry step answered 'nothing to dispatch'."""
    run, d = start(mode="crosslab")
    vias = {v["via"] for v in run["seats"].values()}
    assert vias <= {"cli:claude", "cli:codex", "cli:gemini"} and len(vias) >= 2, vias
    with pytest.raises(runner.RunError) as err:                # nothing has run yet: say how to run it, not "waiting"
        runner.cmd_ingest()
    assert "RUN   council dispatch" in str(err.value)
    victim = "L2"
    monkeypatch.setenv("FAKE_CLI_PROSE", victim)
    assert runner.cmd_dispatch() == 0
    with pytest.raises(runner.RunError) as err:
        runner.cmd_ingest()
    msg = str(err.value)
    assert f"{victim}: REJECTED (output 1): the output is not a JSON object" in msg
    assert "RUN   council dispatch" in msg and f"{victim} via" in msg and "retry" in msg
    capsys.readouterr()
    assert runner.cmd_dispatch() == 0                          # the dead end: this used to print "nothing to dispatch"
    out = capsys.readouterr().out
    assert f"{victim}: ok" in out and "L1: ok" not in out      # only the rejected seat runs again
    runner.cmd_ingest()
    run, _ = current()
    assert run["phase"] == "merge" and run["attempts"] == {f"1/ideate/{victim}": 1}
    monkeypatch.setenv("FAKE_CLI_PROSE", "")                   # the rest of the run flows through the same adapters
    approve_pool(d, 1)
    runner.cmd_ingest()                                        # -> redteam
    runner.cmd_dispatch()
    runner.cmd_ingest()                                        # -> fuse
    fill_fuse(d, 1)
    runner.cmd_ingest()                                        # -> rank
    runner.cmd_dispatch()
    runner.cmd_ingest()                                        # -> elders
    runner.cmd_dispatch()
    runner.cmd_ingest()                                        # -> cut
    run, _ = current()
    assert run["phase"] == "cut" and sum(run["calls"].values()) == 12      # 9 seats + 2 researchers + 1 rejected output
    assert {v["via"] for v in run["seats"].values()} == vias


def test_dispatch_stops_paying_after_two_rejections(fake_clis, monkeypatch, capsys):
    """Red-team S3: with the dead end gone, nothing capped paid re-dispatches."""
    run, d = start(mode="crosslab")
    runner.cmd_dispatch()
    out = d / "loop-1/ideate/L1.out.json"
    for n in (1, 2):
        out.write_text("still not JSON", encoding="utf-8")
        with pytest.raises(runner.RunError):
            runner.cmd_ingest()
        assert current()[0]["attempts"]["1/ideate/L1"] == n
    capsys.readouterr()
    runner.cmd_dispatch()
    text = capsys.readouterr().out
    assert "L1: rejected twice, not sent again" in text and "--drop L1" in text
    runner.cmd_ingest(drop="L1", reason="two outputs in a row were not JSON")
    assert current()[0]["phase"] == "merge"
