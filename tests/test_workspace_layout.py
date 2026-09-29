"""Pure docking operations; the same fixtures drive the browser implementation."""
import copy
import json
from pathlib import Path
import sys

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "lib"))
import workspace_layout as layout  # noqa: E402
import workspace_state  # noqa: E402

CASES = json.loads((ROOT / "tests/fixtures/workspace_layout.json").read_text())
OPS = {"move": layout.move_tab, "detach": layout.detach_tab, "resize": layout.resize_split}


def tab_set(document):
    return sorted(t for g in document["groups"] for t in workspace_state.tab_ids(g["tree"]))


@pytest.mark.parametrize("case", CASES, ids=[c["name"] for c in CASES])
def test_fixture(case):
    original = copy.deepcopy(case["doc"])
    if case.get("error"):
        with pytest.raises(ValueError):
            OPS[case["op"]](case["doc"], *case["args"])
    else:
        out = OPS[case["op"]](case["doc"], *case["args"])
        assert out == case["expect"]
        workspace_state.validate_document(out)
        assert tab_set(out) == tab_set(original)
    assert case["doc"] == original  # operations never mutate their input
