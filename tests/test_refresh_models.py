import sys
from pathlib import Path

import yaml

import refresh_models as rm

SKILL = Path(rm.ROOT)
SNAP = SKILL / "models" / "snapshot-2026-11-13.json"


def resolve():
    policy = yaml.safe_load((SKILL / "models" / "policy.yaml").read_text(encoding="utf-8"))
    models, _ = rm.load_offline(SNAP)
    return rm.resolve(models, policy)


def test_seven_expert_seats_and_crosslab_elders_one_per_lab():
    configs, _ = resolve()
    for profile, fams in configs.items():
        for key, cfg in fams.items():
            if key == "solo":
                continue
            assert len(cfg["experts"]) == 7, (profile, key)
        labs = [m.creator for m in fams["crosslab"]["elders"]]
        assert sorted(labs) == ["Anthropic", "Google", "OpenAI"], profile


def test_restricted_models_are_skipped_without_access():
    configs, _ = resolve()
    picked = {m.name for fams in configs.values() for cfg in fams.values() if isinstance(cfg, dict)
              for role in ("chair", "red_team") for m in [cfg.get(role)] if isinstance(m, rm.Model)}
    assert not any("Carbon" in n for n in picked)


def test_profiles_order_cost_max_ge_balanced_ge_efficient():
    configs, _ = resolve()
    c = {p: configs[p]["crosslab"]["relative_cost"] for p in ("max", "balanced", "efficient")}
    assert c["max"] >= c["balanced"] >= c["efficient"]


def test_open_family_is_open_weights_only():
    configs, _ = resolve()
    cfg = configs["balanced"]["fanout:open"]
    assert all(m.open_weights for m in cfg["experts"] + [cfg["chair"]])


def test_new_model_is_picked_up_without_code_changes():
    policy = yaml.safe_load((SKILL / "models" / "policy.yaml").read_text(encoding="utf-8"))
    models, _ = rm.load_offline(SNAP)
    models.append(rm.Model(name="Claude Future 9 (high)", creator="Anthropic", quality=80, cost=2.0, minutes=5.0))
    configs, _ = rm.resolve(models, policy)
    assert configs["max"]["crosslab"]["chair"].name == "Claude Future 9 (high)"
