# Baseline Fix Report

## Scope

Aligned stale tests with intentional product contracts already present at
`df02e90`. Production files were not changed.

## RED evidence

The required focused commands were run before editing:

```text
pytest -q tests/test_dash_concurrency.py::test_changed_and_stale_pane_models_are_reconciled tests/test_usage_dash.py::test_pane_model_values_normalize_labels_and_preserve_clears
2 failed: pane-model output had `$$$` and colored tier segments that the old assertions omitted.

pytest -q tests/test_dashboard_cards.py
2 failed: itemKind returned `row`, and the old cardCount/urgent-wrap contract was absent.

pytest -q tests/test_usage_ui.py::test_header_has_no_search_nor_open_project
1 failed: the old `#q` search markup was absent after the Ctrl+K switcher migration.
```

## Changes

- `tests/test_usage_dash.py`: assert the exact Claude Sonnet and Codex GPT-5.6
  tmux tier colors/symbols, preserve `%3: None`, and assert the decorated plain
  pane-model lines.
- `tests/test_dash_concurrency.py`: expect the reconciled Codex `$$$` symbol.
- `tests/test_dashboard_cards.py`: cover uniform `row` kinds for the required
  states, the shared `rows`/`rowEl` renderer, waiting-first ordering, and the
  expandable `.rxp` response UI; assert the old card structures are absent.
- `tests/test_usage_ui.py`: assert no old search markup, the Ctrl+K switcher,
  and the new-session button beside the Sessions heading.

## GREEN evidence

The three required focused loops passed after the edits:

```text
2 passed
3 passed
1 passed
```

The full suite passed:

```text
384 passed in 111.70s (0:01:51)
```

`git diff --check` also passed.

## Self-review

- Only the four stale test files were modified; no production behavior was
  altered.
- Assertions target current observable output and structural behavior rather
  than whitespace indentation.
- The cards-free test checks both the uniform row kind and inline expansion
  affordances, while retaining an explicit waiting-first ordering check.

## Concerns

The full suite is relatively slow (about two minutes), but it completed with
no failures. The HTML contract tests intentionally inspect the static dashboard
markup/functions because there is no browser harness in this baseline suite.

## Review follow-up

- The standalone dashboard test guard now invokes all three current test
  functions; the removed card-era names are gone.
- The Ctrl+K switcher assertion now uses tag-level lookaheads, so `id` and
  `title` attribute order cannot invalidate the semantic check.
- Reverification after review fixes:

```text
python3 tests/test_dashboard_cards.py
  passed (exit 0); `python` is unavailable in this shell

pytest -q tests/test_dash_concurrency.py::test_changed_and_stale_pane_models_are_reconciled tests/test_usage_dash.py::test_pane_model_values_normalize_labels_and_preserve_clears
  2 passed
pytest -q tests/test_dashboard_cards.py
  3 passed
pytest -q tests/test_usage_ui.py::test_header_has_no_search_nor_open_project
  1 passed
git diff --check
  passed
```

The full suite was not rerun for these test-only assertion/guard changes;
the preceding full-suite result remains `384 passed`, and the review changes
are confined to the focused test modules.

## Final baseline gate

At HEAD `3f17b3449cb9549deae0d7afef67713cf431218a`, the final full-suite gate
was run once:

```text
$ pytest -q
384 passed in 96.14s (0:01:36)
```

`git diff --check` passed before the final commit amendment. The subsequent
amendment changes only the commit subject; production and test content remain
unchanged.
