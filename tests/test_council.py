import json
from pathlib import Path

import council


def test_tripwires_only_raise_tier():
    assert council.plan("rename a variable", tier="direct")["tier"] == "direct"
    out = council.plan("add a database migration for invoices", tier="direct")
    assert out["tier"] == "full" and "migration" in out["tripwires"]
    assert out["gate"].startswith("GATE tier=full")


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


def test_tally_excludes_self_votes_and_verify_beats_vote():
    mapping = {"A": {"id": "C1", "author": "E1"}, "B": {"id": "C2", "author": "E2"},
               "C": {"id": "C3", "author": "E3"}}
    votes = [{"voter": "E1", "ranking": ["A", "B", "C"]},   # E1's vote for its own A is dropped
             {"voter": "E2", "ranking": ["A", "B", "C"]},
             {"voter": "E3", "ranking": ["A", "B", "C"]}]
    out = council.tally(votes, mapping)
    assert out["self_votes_excluded"] == 3
    assert out["ranking"][0]["label"] == "A"
    out = council.tally(votes, mapping, checks={"A": "fail"})
    assert out["ranking"][-1]["label"] == "A"          # failing a machine check ranks last


def test_cut_keeps_one_pareto_wildcard():
    ranked = ["A", "B", "C", "D", "E", "F", "G"]
    scores = {l: {"R1": 5, "R2": 5} for l in ranked}
    scores["G"]["R2"] = 10                             # G is uniquely best on R2
    out = council.cut(ranked, 5, scores)
    assert "G" in out["survivors"] and len(out["survivors"]) == 5
    assert out["wildcard"]["label"] == "G" and out["wildcard"]["criterion"] == "R2"


def test_check_run_example_passes_and_catches_violations():
    run = json.loads((Path(council.SKILL_DIR) / "templates" / "run.example.json").read_text(encoding="utf-8"))
    assert council.check_run(run)["ok"]
    run["loops"][0]["ideas"]["L1"] = run["loops"][0]["ideas"]["L1"][:5]
    run["loops"] = run["loops"] * 4
    errs = council.check_run(run)["errors"]
    assert any("ideas < 13" in e for e in errs) and any("loops > max" in e for e in errs)


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


def test_check_run_enforces_expert_count():
    base = json.loads((Path(council.SKILL_DIR) / "templates" / "run.example.json").read_text(encoding="utf-8"))
    one_expert = json.loads(json.dumps(base))
    one_expert["loops"][0]["ideas"] = {"L1": [f"idea {i}" for i in range(13)]}
    res_full = council.check_run(one_expert)
    assert not res_full["ok"] and any("experts < 7" in e for e in res_full["errors"])

    lite = json.loads(json.dumps(base))
    lite["tier"] = "lite"
    lite["schedule"] = [3, 1]
    lite["loops"][0]["survivors"] = ["C1"]
    lite["loops"][0]["ideas"] = {f"L{k}": [f"idea {i}" for i in range(5)] for k in (1, 2)}
    res_lite_bad = council.check_run(lite)
    assert not res_lite_bad["ok"] and any("experts < 3" in e for e in res_lite_bad["errors"])

    lite["loops"][0]["ideas"]["L3"] = [f"idea {i}" for i in range(5)]
    assert council.check_run(lite)["ok"]
