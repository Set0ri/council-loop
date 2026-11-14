"""The codebase skeleton and the checks on a written map."""
import json

import codemap

GOOD_MAP = """# Architecture of shop

> ELI5: a small shop that takes orders and bills each one once.

## Purpose and philosophy
The shop takes orders and bills them once. Money code is small, boring and tested. """ + "It stays small. " * 40 + """

## Bird's-eye view
```mermaid
flowchart LR
  cli --> core --> db
```
Orders come in through `shop/cli.py`, are priced by `shop/core.py` and stored by `shop/db.py`.

## Code map
- `shop/core.py`: pricing and totals. It must not talk to the network.
- `shop/db.py`: storage. `web/app.ts` is the browser side and calls `web/api.ts`.
- `tests/`: the tests. Run state lives in `.council/runs/` and each run has a `run.json`.

## Interfaces
Commands `add` and `bill` in `shop/cli.py`. The variable `SHOP_DB` picks the database. See `<your vault>/index.md`.

## Data and control flow
`cli.py` parses, `core.py` prices, `db.py` saves.

## Invariants
An order is billed at most once.

## Extending it
Add a payment method next to `shop/core.py`.

## Performance and cost
One query per order.

## Where to change what
| To do | Start in |
| --- | --- |
| Change prices | `shop/core.py` |
"""


def make_repo(root):
    (root / "shop").mkdir()
    (root / "web").mkdir()
    (root / "tests").mkdir()
    (root / "shop/__init__.py").write_text("", encoding="utf-8")
    (root / "shop/db.py").write_text('"""Storage for orders."""\nimport os\nimport sqlite3\n\nDB = os.environ.get("SHOP_DB")\n\n\n'
                                    'def save(order):\n    """Write one order."""\n    return 1\n\n\ndef _hidden():\n    return 2\n', encoding="utf-8")
    (root / "shop/core.py").write_text('"""Pricing and totals."""\nfrom shop import db\nfrom . import db as again\nimport requests\n\n\n'
                                      'class Order:\n    """One order."""\n    def total(self):\n        return 3\n\n    def _secret(self):\n        return 0\n\n\n'
                                      'def bill(order: Order, retries: int = 2):\n    """Charge an order once."""\n    return db.save(order)\n', encoding="utf-8")
    (root / "shop/cli.py").write_text('"""The command line."""\nimport argparse\nfrom shop.core import bill\n\n\ndef main():\n    ap = argparse.ArgumentParser()\n'
                                     '    sub = ap.add_subparsers()\n    sub.add_parser("add")\n    sub.add_parser("bill")\n\n\nif __name__ == "__main__":\n    main()\n', encoding="utf-8")
    (root / "web/api.ts").write_text("export async function getOrders() { return fetch('/orders') }\nexport const VERSION = 1\n", encoding="utf-8")
    (root / "web/app.ts").write_text("/** The browser app. */\nimport { getOrders } from './api'\nexport class App {}\nconst key = process.env.SHOP_KEY\n", encoding="utf-8")
    (root / "tests/test_core.py").write_text("from shop import core\n\n\ndef test_bill():\n    assert True\n\n\ndef test_total():\n    assert True\n", encoding="utf-8")
    (root / "server.py").write_text('import mcp\n\n@mcp.tool()\ndef shop_bill(order_id: str):\n    """Bill it."""\n    return 1\n\n\n@app.get("/orders")\ndef orders():\n    return []\n', encoding="utf-8")
    (root / "node_modules").mkdir()
    (root / "node_modules/junk.js").write_text("export const x = 1\n", encoding="utf-8")
    (root / "README.md").write_text("# shop\n", encoding="utf-8")


def test_skeleton_lists_symbols_interfaces_and_who_leans_on_whom(proj):
    make_repo(proj)
    sk = codemap.skeleton(proj)
    md, files = sk["markdown"], sk["scanned"]["files"]
    assert "node_modules/junk.js" not in files and files["shop/core.py"]["doc"] == "Pricing and totals."
    names = [s["name"] for s in files["shop/core.py"]["symbols"]]
    assert names == ["Order", "bill"] and files["shop/core.py"]["symbols"][0]["methods"] == ["total"]
    edges = codemap.resolve_imports(files)
    assert edges["shop/core.py"] == ["shop/db.py"] and edges["shop/cli.py"] == ["shop/core.py"] and edges["web/app.ts"] == ["web/api.ts"]
    assert edges["tests/test_core.py"] == ["shop/core.py"]
    ranked = sorted((n for n in files if files[n]["code"]), key=lambda n: -codemap.rank(files, edges)[n])
    assert ranked[0] == "shop/db.py" and ranked[1] == "shop/core.py"        # everything leans on storage, then pricing
    assert "Commands in `shop/cli.py`: add, bill" in md and "tool shop_bill" in md and "GET /orders" in md
    assert "`SHOP_DB`" in md and "`SHOP_KEY`" in md and "Runnable files (entry points): `shop/cli.py`" in md
    assert "`bill(order: Order, retries: int=2)` · used in 1 other file: Charge an order once." in md
    assert "_hidden" not in md and "_secret" not in md
    assert "requests (1)" in md and "sqlite3" not in md                      # the standard library is not a dependency
    assert "2 test functions in 1 files" in md and sk["stats"]["code_files"] == 8


def test_map_goes_stale_only_when_the_structure_changes(proj):
    make_repo(proj)
    st = codemap.stale(proj)
    assert st["stale"] and "no map yet" in st["reason"]
    path, note = codemap.install_map(GOOD_MAP, proj, run="run-1")
    assert path == proj / "docs" / "ARCHITECTURE.md" and note is None and path.read_text(encoding="utf-8").startswith(codemap.MARKER)
    assert not codemap.stale(proj)["stale"]
    core = proj / "shop/core.py"
    core.write_text(core.read_text(encoding="utf-8").replace("return db.save(order)", "x = 1\n    return db.save(order)"), encoding="utf-8")
    assert not codemap.stale(proj)["stale"], "a change inside a function body is not a change of structure"
    (proj / "README.md").write_text("# shop, renamed\n", encoding="utf-8")
    assert not codemap.stale(proj)["stale"], "prose does not make the map stale (and the map itself is prose)"
    core.write_text(core.read_text(encoding="utf-8").replace("retries: int = 2", "retries: int = 2, dry_run: bool = False"), encoding="utf-8")
    st = codemap.stale(proj)
    assert st["stale"] and st["diff"]["changed"] == ["shop/core.py"]
    assert st["reason"].endswith(": shop/core.py"), "the reason names the files, so nobody has to hunt for them"
    (proj / "shop/refunds.py").write_text("def refund():\n    return 0\n", encoding="utf-8")
    assert codemap.stale(proj)["diff"]["added"] == ["shop/refunds.py"]
    assert "new files: `shop/refunds.py`" in codemap.skeleton(proj)["markdown"]
    assert codemap.cmd_map(check=True) == 1 and codemap.cmd_map(accept_now=True) == 0 and codemap.cmd_map(check=True) == 0
    assert json.loads((proj / ".council/map/state.json").read_text(encoding="utf-8"))["structure"]["shop/refunds.py"]


def test_a_new_test_is_not_a_new_structure(proj):
    make_repo(proj)
    codemap.install_map(GOOD_MAP, proj, run="run-1")
    tests = proj / "tests/test_core.py"
    tests.write_text(tests.read_text(encoding="utf-8") + "\n\ndef test_refund():\n    assert True\n\n\nclass TestTotals:\n    def test_sum(self):\n        assert True\n", encoding="utf-8")
    assert not codemap.stale(proj)["stale"], "writing a test must not cost a run of the mapper"
    tests.write_text(tests.read_text(encoding="utf-8") + "\n\ndef make_order():\n    return {}\n", encoding="utf-8")
    st = codemap.stale(proj)
    assert st["stale"] and st["diff"]["changed"] == ["tests/test_core.py"], "a new helper in a test file still counts"
    codemap.accept(proj)
    core = proj / "shop/core.py"
    core.write_text(core.read_text(encoding="utf-8") + "\n\ndef test_connection():\n    return True\n", encoding="utf-8")
    assert codemap.stale(proj)["stale"], "outside test files a name that starts with test_ is an ordinary public name"
    for name, yes in {"tests/test_core.py": True, "pkg/queue_test.go": True, "web/cart.spec.ts": True, "src/__tests__/a.js": True,
                      "conftest.py": True, "shop/core.py": False, "shop/contest.py": False, "attest/keys.py": False}.items():
        assert bool(codemap.TEST_FILE_RX.search(name)) is yes, name


def test_project_without_code_needs_no_map(proj):
    st = codemap.stale(proj)
    assert st == {"applies": False, "stale": False, "reason": "no source code found in this project", "path": st["path"]}


def test_validate_map_checks_sections_and_that_named_paths_exist(proj):
    make_repo(proj)
    errs, warns = codemap.validate_map(GOOD_MAP, proj)
    assert errs == [] and warns == []                       # `.council/…`, bare `run.json` and placeholders are not checked
    errs, _ = codemap.validate_map(GOOD_MAP.replace("## Invariants", "## Rules").replace("## Extending it", "## Growth"), proj)
    assert errs == ["missing section: ## Invariants", "missing section: ## Extending it"]
    invented = GOOD_MAP + "\nSee `shop/payments/stripe.py`, `shop/ledger.py`, `web/checkout.tsx` and `billing.py`.\n"
    errs, _ = codemap.validate_map(invented, proj)
    assert len(errs) == 1 and "shop/ledger.py" in errs[0] and "billing.py" in errs[0] and "shop/core.py" not in errs[0]
    _, warns = codemap.validate_map(GOOD_MAP + "\nAlso `shop/ghost.py`.\n", proj)
    assert warns == ["named in the map but not found: shop/ghost.py"]       # one stray name warns; three or more is an error
    assert codemap.validate_map(GOOD_MAP.replace("> ELI5:", "> Summary:"), proj)[1] == ["no ELI5 line under the title"]
    assert codemap.validate_map("## Purpose and philosophy\nshort", proj)[0][0].startswith("the map is 5 words")
    assert codemap.named_paths("`--drop`, `council.py next`, `https://x.org/a/b`, `skill://a/b.md`, `tests/`, `core.py`") == ["core.py", "tests/"]


def test_install_never_overwrites_a_map_someone_wrote_by_hand(proj):
    make_repo(proj)
    mine = proj / "docs" / "ARCHITECTURE.md"
    mine.write_text("# My own notes\nDo not touch.\n", encoding="utf-8")
    path, note = codemap.install_map(GOOD_MAP, proj)
    assert path.name == "ARCHITECTURE.council.md" and "left alone" in note and mine.read_text(encoding="utf-8").startswith("# My own notes")
    mine.unlink()
    first, _ = codemap.install_map(GOOD_MAP, proj)
    again, note = codemap.install_map(GOOD_MAP.replace("billed at most once", "billed exactly once"), proj)
    assert first == again == mine and note is None and "exactly once" in mine.read_text(encoding="utf-8")      # its own map may be replaced
    assert mine.read_text(encoding="utf-8").count(codemap.MARKER) == 1


def test_a_new_language_is_one_registered_function(proj):
    (proj / "main.zig").write_text('pub fn start() void {}\n', encoding="utf-8")
    codemap.CODE[".zig"] = "Zig"
    codemap.register([".zig"], lambda text: {"doc": "", "symbols": [{"name": "start", "kind": "fn", "sig": "pub fn start()", "doc": "", "line": 1}],
                                             "imports": [], "main": True})
    try:
        md = codemap.skeleton(proj)["markdown"]
        assert "`pub fn start()`" in md and "| Zig | 1 | 1 |" in md
    finally:
        codemap.CODE.pop(".zig")
        codemap.EXTRACTORS.pop(".zig")


def test_a_map_can_be_installed_by_hand_and_a_bad_one_is_refused(proj, capsys):
    make_repo(proj)
    draft = proj / "draft.md"
    draft.write_text("```markdown\n" + GOOD_MAP + "\n```\n", encoding="utf-8")                 # a fence around the whole document is dropped
    assert codemap.cmd_map(install=str(draft)) == 0
    placed = (proj / "docs" / "ARCHITECTURE.md").read_text(encoding="utf-8")
    assert placed.startswith(codemap.MARKER) and "by hand (council map --install)" in placed.splitlines()[0]
    assert "```markdown" not in placed and not codemap.stale(proj)["stale"]
    draft.write_text(GOOD_MAP + "\n`shop/a.py` `shop/b.py` `shop/c.py`\n", encoding="utf-8")
    assert codemap.cmd_map(install=str(draft)) == 1 and "not installed" in capsys.readouterr().out
    assert "shop/a.py" not in (proj / "docs" / "ARCHITECTURE.md").read_text(encoding="utf-8")


def test_ranking_follows_uses_of_names_and_ignores_comments_and_strings(proj):
    """The method of aider's repo map: an edge from the file that uses a name to the file that defines it."""
    make_repo(proj)
    files = codemap.scan(proj)["files"]
    assert "orders" not in files["shop/db.py"]["uses"]           # "Storage for orders." is a docstring, not a use of orders()
    assert "orders" not in files["web/api.ts"]["uses"]           # fetch('/orders') is a string
    assert files["shop/cli.py"]["uses"] == {"bill": 1, "main": 1}
    res = codemap.ranking(files, codemap.resolve_imports(files))
    assert res["used_by"][("shop/core.py", "bill")] == 1 and res["used_by"][("web/api.ts", "getOrders")] == 1
    assert res["names"][("shop/db.py", "save")] > res["names"][("server.py", "orders")]    # used beats unused
    assert res["score"]["server.py"] < res["score"]["shop/core.py"]         # a file nothing uses does not rise by being isolated
    assert codemap.well_named("get_orders") and codemap.well_named("getOrders") and not codemap.well_named("bill")


def test_focus_map_puts_what_the_ask_names_first_and_keeps_to_its_budget(proj):
    make_repo(proj)
    fm = codemap.focus_map(proj, "Change how bill charges an Order; start in shop/cli.py", budget_tokens=120)
    lines = fm["markdown"].splitlines()
    assert lines[0] == "- `shop/cli.py` (named in the ask)" and fm["mentioned"] == ["Order", "bill", "shop"]
    assert lines[2] == "- `shop/core.py`" and "bill(order: Order" in lines[3] and "class Order" in lines[4]
    assert fm["tokens"] <= 120
    small = codemap.focus_map(proj, "anything", budget_tokens=30)
    assert 0 < small["tokens"] <= 30 and len(small["markdown"].splitlines()) < len(lines)
    assert codemap.focus_map(proj / "docs", "x")["markdown"] == ""            # no code, no map


def test_tree_sitter_is_optional_and_never_changes_what_counts_as_structure(proj, monkeypatch):
    pack, have = codemap.tree_sitter()
    if not pack or "go" not in have:
        import pytest
        pytest.skip("tree-sitter-language-pack with the Go parser is not on this machine")
    (proj / "ledger.go").write_text('package billing\n\nimport "fmt"\n\n// Ledger keeps the events.\ntype Ledger struct{ n int }\n\n'
                                    'func (l *Ledger) Write(id string) (bool, error) { fmt.Println(id); return true, nil }\n\n'
                                    'func helper() int { return 1 }\n', encoding="utf-8")
    with_ts = codemap.scan(proj)
    rich = {s["name"]: s for s in with_ts["files"]["ledger.go"]["rich"]}
    assert with_ts["parsers"] == "tree-sitter" and set(rich) == {"Write", "Ledger"}       # helper is not exported
    assert rich["Write"]["sig"] == "func (l *Ledger) Write(id string) (bool, error)"       # the full signature, receiver included
    monkeypatch.setattr(codemap, "_TS", (None, set()))
    without = codemap.scan(proj)
    assert without["parsers"] == "built-in" and "rich" not in without["files"]["ledger.go"]
    assert codemap.structure(with_ts) == codemap.structure(without)          # two machines agree on staleness


def test_generic_reader_covers_languages_without_their_own(proj):
    (proj / "q.c").write_text('#include "q.h"\nstatic int depth = 0;\n\nint queue_push(struct queue *q, int v)\n{\n    if (v) {\n        return 1;\n    }\n    return 0;\n}\n\nstruct queue {\n    int n;\n};\n', encoding="utf-8")
    (proj / "schema.sql").write_text("CREATE TABLE IF NOT EXISTS orders (id int);\ncreate or replace view open_orders as select 1;\n", encoding="utf-8")
    files = codemap.scan(proj)["files"]
    assert [s["name"] for s in files["q.c"]["symbols"]] == ["queue_push", "queue"] and files["q.c"]["imports"] == ["q.h"]
    assert [(s["kind"], s["name"]) for s in files["schema.sql"]["symbols"]] == [("TABLE", "orders"), ("view", "open_orders")]
