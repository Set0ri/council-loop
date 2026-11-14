import json
from pathlib import Path

import pytest

import council


def test_tripwires_only_raise_tier():
    assert council.plan("rename a variable", tier="direct")["tier"] == "direct"
    out = council.plan("add a database migration for invoices", tier="direct")
    assert out["tier"] == "full" and "migration" in out["tripwires"]
    assert out["gate"].startswith("GATE tier=full")


def test_tripwires_fire_on_risk_and_stay_quiet_on_chores():
    hit = council.tripwires_hit
    assert "migration" in hit("migrate the auth database to Postgres") and "auth/security" in hit("migrate the auth database")
    assert "data loss/irreversible" in hit("drop the customers table")
    for chore in ("delete unused import", "rename a variable", "migrate the docs site to a new theme", "tidy the changelog"):
        assert hit(chore) == [], chore


def test_blind_hides_authors_and_enforces_caps():
    cards = [{"id": f"C{i}", "author": f"E{i}", "title": f"t{i}", "body": "word " * 50, "eli5": "simple"}
             for i in range(7)]
    out = council.blind(cards, seed=3)
    assert out["ok"]
    assert "E1" not in out["packet"] and "C1" not in out["packet"]
    assert sorted(m["id"] for m in out["mapping"].values()) == sorted(c["id"] for c in cards)
    # same seed -> same order; different seed -> (very likely) different order
    assert council.blind(cards, seed=3)["mapping"] == out["mapping"]
    cards[0]["body"] = "word " * 200
    assert not council.blind(cards)["ok"]


def _map(**authors):
    return {l: {"id": f"C{i + 1}", "authors": a} for i, (l, a) in enumerate(authors.items())}


def test_tally_equals_borda_on_complete_ballots_and_verify_beats_vote():
    mapping = _map(A=[], B=[], C=[])
    votes = [{"voter": f"L{i}", "ranking": r} for i, r in enumerate((["A", "B", "C"], ["A", "C", "B"], ["B", "A", "C"]))]
    out = council.tally(votes, mapping)
    assert [r["label"] for r in out["ranking"]] == ["A", "B", "C"]          # Borda: A 5, B 3, C 1
    assert out["recused"] == 0 and out["head_to_heads"] == 9 and out["thin"] == []
    assert council.tally(votes, mapping, checks={"A": "fail"})["ranking"][-1]["label"] == "A"
    assert council.tally(votes, mapping, kills=["A"])["ranking"][-1]["killed"]


def test_tally_voter_sits_out_head_to_heads_with_a_stake_on_one_side():
    mapping = _map(A=["L1"], B=["L2"], C=["L3"])
    votes = [{"voter": v, "ranking": ["A", "B", "C"]} for v in ("L1", "L2", "L3")]
    out = council.tally(votes, mapping)
    # L1 may only judge B vs C, L2 only A vs C, L3 only A vs B: one judge per head-to-head
    assert out["head_to_heads"] == 3 and out["recused"] == 6
    assert [r["label"] for r in out["ranking"]] == ["A", "B", "C"]
    assert out["agreement"] is None and "not measurable" in out["agreement_note"]


def test_tally_cowritten_cards_still_get_a_fair_count():
    """Red-team finding, run 261113-183103: three two-author candidates used to score 0.0 each."""
    mapping = _map(A=["X", "Y"], B=["Y", "Z"], C=["X", "Z"])
    fwd = council.tally([{"voter": v, "ranking": ["A", "B", "C"]} for v in "XYZ"], mapping)
    rev = council.tally([{"voter": v, "ranking": ["C", "B", "A"]} for v in "XYZ"], mapping)
    assert [r["label"] for r in fwd["ranking"]] == ["A", "B", "C"]
    assert [r["label"] for r in rev["ranking"]] == ["C", "B", "A"]          # the ballots decide, not the label order
    everyone = council.tally([{"voter": v, "ranking": ["A", "B"]} for v in "XYZ"], _map(A=["X", "Y", "Z"], B=["X"]))
    assert everyone["ranking"][0]["label"] == "A" and everyone["ranking"][0]["score"] > 0.5


def test_tally_gap_is_no_opinion_and_labels_are_cleaned():
    """Red-team finding: a ballot that skipped or misspelt its top pick used to vote that card last."""
    mapping = _map(**{l: [] for l in "ABCDE"})
    full = [{"voter": f"L{i}", "ranking": list("ABCDE")} for i in range(4)]
    skipped = full + [{"voter": "L9", "ranking": list("BCDE")}]             # forgot A
    assert council.tally(skipped, mapping)["ranking"][0]["label"] == "A"
    assert council.tally(skipped, mapping)["gaps"] == 1
    spelt = full + [{"voter": "L9", "ranking": ["Candidate A", "b", "C.", "D", "E"]}]
    out = council.tally(spelt, mapping)
    assert out["problems"] == [] and out["gaps"] == 0 and out["ranking"][0]["label"] == "A"
    once = council.tally([{"voter": "L8", "ranking": ["E", "A"]}] + full, mapping)     # one fan cannot crown a card
    assert once["ranking"][0]["label"] == "A"


def test_tally_agreement_is_one_when_voters_agree_and_near_zero_when_split():
    mapping = _map(**{l: [] for l in "ABCD"})
    same = council.tally([{"voter": f"L{i}", "ranking": list("ABCD")} for i in range(5)], mapping)
    split = council.tally([{"voter": f"L{i}", "ranking": list("ABCD") if i % 2 else list("DCBA")} for i in range(6)], mapping)
    assert same["agreement"] == 1.0 and same["agreement_note"] == "strong"
    assert split["agreement"] < 0.2 and split["agreement_note"].startswith("weak")


def test_tally_weights_by_stake_so_a_card_many_voters_fed_is_still_judged():
    """Elder S1, run 261113-183103: a favourite written by 6 of 7 seats, first on every ballot, must lead."""
    voters = [f"L{i}" for i in range(1, 8)]
    mapping = {"A": {"id": "X", "authors": voters[:6]},
               **{l: {"id": l, "authors": [voters[i]]} for i, l in enumerate("BCDEFG")}}
    out = council.tally([{"voter": v, "ranking": list("ABCDEFG")} for v in voters], mapping)
    assert out["ranking"][0]["label"] == "A" and out["thin"] == []
    assert council.stake("L1", mapping["A"]) == pytest.approx(1 / 6) and council.stake("L7", mapping["A"]) == 0
    assert council.stake("L1", mapping["B"]) == 1                       # a sole author's own head-to-heads count for nothing
    sole = council.tally([{"voter": "L1", "ranking": ["B", "C"]}], mapping)
    assert sole["recused"] == 1 and sole["head_to_heads"] == 0


def test_protected_keeps_what_the_vote_could_not_judge():
    mapping = {l: {"id": l, "authors": [a]} for l, a in zip("ABC", ("L1", "L2", "L3"))}
    out = council.tally([{"voter": v, "ranking": list("ABC")} for v in ("L1", "L2")], mapping)   # L3 never voted
    assert out["thin"] == [["A", "B"]]                       # both sole authors sat it out: nobody judged A against B
    order = [r["label"] for r in out["ranking"]]
    assert council.protected(order, 1, out["thin"]) == [order[1]]
    assert council.protected(order, 1, out["thin"], barred={order[1]}) == []
    assert council.protected(order, 2, out["thin"]) == []


def test_seat_orders_are_evenly_spaced():
    """Red-team S2: rotating by one left 21 cards at mean positions 3 to 17."""
    labels = council.labels_for(21)
    orders = council.seat_orders(labels, [f"L{i}" for i in range(1, 8)])
    assert len({o[0] for o in orders.values()}) == 7 and all(sorted(o) == sorted(labels) for o in orders.values())
    for l in labels:
        mean = sum(o.index(l) for o in orders.values()) / 7
        assert abs(mean - 10) <= 21 / 7, (l, mean)           # every card's mean position is near the middle
    three = council.seat_orders(["A", "B", "C"], ["L1", "L2", "L3"])
    assert sorted(o[0] for o in three.values()) == ["A", "B", "C"]
    assert council.seat_orders(["A", "B"], ["EA"]) == {"EA": ["A", "B"]}


def test_leak_check_flags_identity_and_not_ordinary_words():
    """Card L3#2: the check fired on 'seat call' and 'seat cost', five of five tried."""
    def errs(text):
        card = {"id": "C1", "authors": ["L2"], "title": "t", "body": text + " word" * 80, "eli5": "simple"}
        return council.blind([card], forbidden=["L1", "L2", "L3"])["errors"]
    for fine in ("each seat call costs tokens", "a seat and its vote", "use the rubric as a lens", "seat ids are checked",
                 "treat it as an expert would", "the elder agreed", "seat cost and seat vote and seat gets"):
        assert errs(fine) == [], fine
    for leak in ("from my lens this wins", "as the cost expert I prefer this", "seat L3 proposed this", "L2 wrote this card",
                 "our idea is better", "elder EA will like it", "my own card says so"):
        assert errs(leak), leak


def test_clean_label_and_token_estimate():
    labels = ["A", "B", "AA"]
    assert [council.clean_label(x, labels) for x in ("A", " b ", "Candidate B", "candidate: aa", "B.", "Z", "", None, "A or B")] == \
        ["A", "B", "B", "AA", "B", None, None, None, None]
    measured = {"measured_tokens_per_call": {"ideate": 150000, "control": 150000, "vote": 64000, "rank": 64000,
                                             "redteam": 240000, "elders": 64000, "elders_verify": 240000}}
    assert council.estimate_tokens("lite", 1, measured) == 1_272_000                    # the measured Lite run was 1,269,206
    assert council.estimate_tokens("full", 3, measured, "max") == 8_868_000
    assert council.estimate_tokens("full", 3, measured, "balanced") == 7_284_000        # elders judge the packet only
    assert council.estimate_tokens("direct", 0, measured) is None and council.estimate_tokens("full", 3, {}) is None
    assert council.estimate_calls("full", 3) == 76 and council.estimate_calls("lite", 1) == 9


def test_cut_keeps_one_pareto_wildcard():
    ranked = ["A", "B", "C", "D", "E", "F", "G"]
    scores = {l: {"R1": 5, "R2": 5} for l in ranked}
    scores["G"]["R2"] = 10                             # G is uniquely best on R2
    out = council.cut(ranked, 5, scores)
    assert "G" in out["survivors"] and len(out["survivors"]) == 5
    assert out["wildcard"]["label"] == "G" and out["wildcard"]["criterion"] == "R2"


def test_check_run_example_passes_and_catches_violations():
    run = json.loads((Path(council.SKILL_DIR) / "templates" / "run.example.json").read_text(encoding="utf-8"))
    assert run["tier"] == "lite" and council.check_run(run)["ok"]
    run["loops"][0]["ideas"]["L1"] = run["loops"][0]["ideas"]["L1"][:2]
    run["loops"][0]["red_team_skipped"] = "the red team seat was dropped"
    run["loops"] = run["loops"] * 4
    errs = council.check_run(run)["errors"]
    assert any("ideas < 5" in e for e in errs) and any("loops > max" in e for e in errs)
    assert any("no red-team step" in e for e in errs)


def test_locks_detect_changes(tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    (tmp_path / "tests").mkdir()
    (tmp_path / "tests" / "test_x.py").write_text("assert True\n", encoding="utf-8")
    council.lock_files(["tests"])
    assert council.verify_locks()["ok"]
    (tmp_path / "tests" / "test_x.py").write_text("assert 1\n", encoding="utf-8")
    out = council.verify_locks()
    assert not out["ok"] and out["changed"] == ["tests/test_x.py"]


def test_models_for_every_mode_and_profile():
    for profile in council.PROFILES:
        for mode in ["solo", "crosslab"] + [f"fanout:{f}" for f in council.FAMILIES]:
            out = council.models_for(mode, profile)
            assert "error" not in out, (mode, profile, out)
            assert "chair" in out and "experts" in out
