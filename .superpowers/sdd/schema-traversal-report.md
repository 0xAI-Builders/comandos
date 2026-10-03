# Ordered schema traversal dependency audit

The private evaluator preserves installed Python traversal order, consumption and supported abort categories on the measured semantic cohort. Every unimplemented reached operation remains a named ScopeGap. The complete directed and full audits still exit 1.

The stock worker remains unwired to both preflight and traversal. Its measured outcomes are unchanged on every approved corpus row. The test-only preflight composition retains existing schema-stage false rejects and loses coverage where traversal has explicit gaps.

These measurements were captured on 2026-10-03 from the isolated candidate whose source and evidence hashes are frozen in /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.superpowers/sdd/schema-traversal-frozen.json. The approved base is 263492fb996b5e782b2dc5ccd8c2dc36901c100f. The separate incoming reference branch is outside this diff.

## Measured boundary

The audit records original JSON strings, root selection, exact Python schema checking, native preflight, exact instance outcome, traversal, stock worker and composition for every row. It invokes traversal only after exact installed selected-class check_schema success. Native preflight success cannot establish that precondition.

| Measurement | Rows | Exact semantic results | Scope/protective gaps | Precondition omissions | Traversal category discrepancies | Selector discrepancies |
| --- | --- | --- | --- | --- | --- | --- |
| Directed | 227 | 178 | 33 | 16 | 0 | 0 |
| Approved full corpus | 735 | 290 | 208 | 237 | 0 | 0 |

The supported directed cohort has no unexpected gap and no missing declared gap. The native probes have zero infrastructure failures and zero timeouts. Schema-invalid rows remain visible as not-run:precondition; their preflight and stock results remain measured.

| Comparison | Acceptance differences | Classification differences | New false accepts | New false rejects against stock | Lost matching coverage |
| --- | --- | --- | --- | --- | --- |
| Rebuilt unchanged stock, approved full corpus | 60 | 192 | 0 | 0 | 0 |
| Native schema stage, separate original cohort | 31 of 184 | Acceptance only | Unchanged | Unchanged | Not applicable |
| Preflight plus traversal, directed | 1 false reject, 0 false accepts | Typed results retained | 0 | 0 | 12 |
| Preflight plus traversal, full | 16 false rejects, 0 false accepts | Typed results retained | 0 | 4 existing regex cases | 194 |

The full preflight comparison has 38 acceptance differences across its schema inventory. All original separate schema-stage rows and all full preflight statuses match their approved per-row outcomes. The four composition regressions are exactly the previously documented regex cases:

- original105/regex-python-named-group
- original105/regex-inline-flags
- schema184/draft201909-pattern-python-inline
- schema184/draft202012-pattern-python-inline

Lost coverage counts previously Python/stock-matching rows that now encounter a traversal gap or an unresolved composition precondition. A scope gap never counts as ordinary Invalid or as a repaired acceptance mismatch. The tables are derived from the machine-readable directed, full and invariance artifacts listed below.

## Implementation and semantic exclusions

The evaluator accepts immutable original schema and instance Values through a sibling-private evaluate_checked API. The draft must match Python root selection. The exact schema-check precondition remains test-only proof; runtime code neither imports Python nor recovers through Python.

/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-extensions/src/output_schema/traversal.rs separates executing class, applicable-keyword filter owner and resolver context. Local registry handles retain a resource specification separately. Ordered handlers implement booleans, required, nonnumeric type, properties, allOf, anyOf, oneOf, not, if, supported contains counters, lazy ref and disabled formats.

Only certified one-key required/type fragments reach the public offline jsonschema 0.58.4 compiler. The fragment cache is local to one evaluation and keyed by original node, class and keyword. An eligible build failure remains InternalFragmentCompilation. No arbitrary schema or referenced subtree reaches stock compilation.

The installed keyword inventory is checked per class. Unknown extensions remain ignored; recognized unimplemented keywords produce named gaps when applicable. Unvisited properties and skipped branches produce no speculative gaps. Literal default/examples data and original parsed number values remain unchanged; reached const/enum remain Keyword gaps.

/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-extensions/src/output_schema/traversal/local_refs.rs implements empty-base local pointers, static anchors and proven missing resources without I/O. Percent decoding precedes pointer splitting; plus signs, malformed escapes, tilde replacements, signed ASCII array indexes and leading zeros retain their directed source behavior. Invalid UTF-8, exotic Python integer spellings and scalar/string traversal remain PointerSemantics gaps.

Lazy crawling follows the specification inventories, including older items/dependencies rules. Mixed legacy dependencies can yield arrays as resources; those rows retain Python AttributeError and native ResourceId gap. Resource-specification detection during crawling remains distinct from validator evolution. Noncanonical or unknown crawl declarations remain ResourceDialect gaps.

A unique empty effective ID can replace the registry's empty-URI resource. Immutable resolver snapshots preserve that replacement for later lookup while retaining the original calling context. Same-container competing registrations follow the source's deterministic insertion/LIFO order. Competing registrations across source containers remain ResourceId gaps because Python set order is outside the claimed boundary.

The approved bundled resource URI inventory is checked against the installed registry. Resource ID registration removes trailing hash characters, preserving historical HTTP spellings. The Draft4 HTTP resource is an ExternalResource gap; its unregistered HTTPS spelling is proven absent after supported local crawling. Nonfragment references containing a hash remain UriJoin gaps.

The reached modern-parent/Draft4-child required-empty-array case remains FragmentShape. Python's modern root schema checker accepts it, while Draft4 fragment compilation cannot certify that shape. The evaluator reports the gap before compilation and never converts it into Invalid or an internal build failure.

The new sibling selector preserves caller fallback for unknown declarations and Python's membership-first behavior for strings/lists. It returns coarse DialectFailure for unsupported selector failures. The original worker selector and compilation-clone APIs retain their behavior. Exact selector results and schema-stage failures for nonmapping roots are frozen separately.

Named exclusions also include Draft3, numeric/equality semantics, Python regex, unsupported assertions, annotations, dynamic scope, nonempty resource IDs, external-resource evaluation and general URI joining. /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.superpowers/sdd/schema-traversal-invariance.json lists every directed/full gap category and count.

## Cycle proof and resource measurements

Active frames include schema identity, instance location identity, validator class, filter owner, resolver context and consumption mode. An identical active frame produces RecursionError because evaluation cannot progress. Every exit removes its frame. Productive recursion uses distinct instance locations; no global visited-pair success cache exists.

The installed oracle returns Valid at productive depths 1, 8, 24, 48, 63, 64, 96 and 160. Native JSON traversal returns Valid through the measured depth 63 boundary. At depth 64 the evaluator's active-frame guard yields BudgetBoundary; depth 96 repeats that result. Depth160 exceeds native JSON transport parsing and is reported as BudgetBoundary with phase=input.

Directly constructed Values independently validate at depth 48 and return a protective gap at depth 260. These direct values do not repair JSON transport coverage. Root and two-node no-progress cycles, including an unknown-root dialect, retain the observed RecursionError category.

The first 512-frame guard allowed a Rust test-thread stack overflow at depth 160. /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.superpowers/sdd/schema-traversal-green-attempt.log retains that failed run. The guard is now 128 active frames; stack overflow is not counted as a semantic pass or inferred Python recursion.

Native probes apply the existing 384 MiB address-space limit and 2/3 second CPU limits before reading input. Each batch contains at most eight rows; probe input is capped at 8 MiB. Names are capped at 512 bytes and emitted gap pointers at 4096 characters. Additional guards cap evaluation operations at 100000 and crawl resources/depth at 100000/512.

| Final native probe measurement | Batches | Maximum wall seconds | Maximum CPU seconds | Summed CPU seconds | Cumulative child peak RSS KiB |
| --- | --- | --- | --- | --- | --- |
| Directed schema stage | 29 | 0.1060 | 0.1055 | 1.1099 | 30524 |
| Directed traversal | 29 | 0.0642 | 0.0634 | 0.4364 | 30524 |
| Full schema stage | 92 | 0.1181 | 0.1178 | 2.7653 | 31040 |
| Full traversal | 92 | 0.0597 | 0.0593 | 0.8984 | 31264 |

RSS comes from RUSAGE_CHILDREN and is explicitly cumulative within each audit process. It is not a per-row RSS bound. Detailed batch measurements remain in the directed/full artifacts. General all-input resource parity remains unproved.

## Test-first evidence and regression checks

The original directed oracle capture preceded evaluator code. The first Rust test failed because the required selector/evaluator APIs were absent, matching the brief's permitted API-absence RED. The empty-ID registry fixture then produced a behavioral RED: native Invalid against Python Valid. Immutable resolver snapshots corrected that discrepancy.

Self-review initially conflated validator selection with schema checking for nonmapping roots. Installed source and the separate oracle showed membership-first fallback for ordinary strings/lists, TypeError for null/numbers, and later indexing failure when nonmapping membership finds the declaration. A corrected failing test preceded the sibling-selector fix.

The first clippy run identified one collapsible conditional. Formatting and clippy passed after its mechanical correction. The unsupported fragment-shape and protective depth additions follow explicit controller clarification in /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.superpowers/sdd/schema-traversal-brief.md.

| Required check | Result |
| --- | --- |
| Focused traversal Rust tests | 5 passed, 1 ignored explicit audit driver |
| Cargo formatting check | Passed |
| Clippy, all targets, deny warnings | Passed |
| Full Rust crate tests | 112 passed, 2 ignored explicit audit drivers |
| Ordinary Python tests | 417 passed |
| Isolated release build | Passed |
| Directed/full differential audits | Exit 1 with retained declared gaps |
| Stock full audit | Exit 1 with unchanged approved differences |
| Separate schema-stage audit | Exit 1 with unchanged approved gaps |
| Audit-process file/network markers | Passed |
| Exact per-row and protected-file invariance | Passed |

The full Rust suite includes existing worker deadline, lifecycle, asynchronous progress and child-reaping tests. Native evaluator and audit-process marker checks leave an isolated private-file access marker and loopback listener untouched. The audit-process check covers exact Python, preflight, traversal and stock subprocesses.

The pre-selector captures are retained at /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.superpowers/sdd/schema-traversal-directed-pre-selector.json and /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.superpowers/sdd/schema-traversal-full-pre-selector.json. Their frozen hashes remain at /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.superpowers/sdd/schema-traversal-frozen-pre-selector.json. The final focused, formatting, clippy, full Rust, release and dependent differential checks ran after the selector correction. Ordinary pytest is the earlier passing run; the selector change adds only private Rust behavior and explicit audit fixtures.

One cached Cargo no-run artifact-discovery command overlapped ordinary pytest for 0.025 seconds. It performed no compilation and produced no observed interference. Other final regression/audit steps were serialized under the sole Cargo/test owner. No browser, session, service, dependency or budget change occurred.

## Exact execution commands

All commands use the local worktree /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration. Paths beginning /work, /venv or /tmp in these commands/artifacts belong only to isolated containers. The oracle runner mounts /home/someguy/.local/share/comandos/extensions-venv and /home/someguy/.cache/comandos/tiktoken read-only.

```sh
cd /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/scripts/rust-sandbox cargo test -p comandos-extensions --lib output_schema::traversal -- --nocapture
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/scripts/rust-sandbox cargo fmt --all -- --check
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/scripts/rust-sandbox cargo clippy -p comandos-extensions --all-targets -- -D warnings
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/scripts/rust-sandbox cargo test -p comandos-extensions
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/scripts/rust-sandbox python3 -m pytest /work/crates/comandos-extensions/tests -q
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/scripts/rust-sandbox cargo build --release -p comandos-extensions
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/scripts/rust-sandbox cargo test -p comandos-extensions --lib --no-run --message-format=json
```

The actual library-test executable was discovered from the final Cargo compiler-artifact output at /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.superpowers/sdd/schema-traversal-build-artifacts.jsonl. Its container-only path is /work/.migration-build/target/debug/deps/comandos_extensions-e067645736155fb7; the local binary is /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.migration-build/target/debug/deps/comandos_extensions-e067645736155fb7. Reproduction must rediscover it if Cargo metadata changes.

```sh
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/scripts/rust-python-oracle /home/someguy/.local/share/comandos/extensions-venv /home/someguy/.cache/comandos/tiktoken python /work/crates/comandos-extensions/tests/output_schema_traversal_audit.py --stage-binary /work/.migration-build/target/debug/deps/comandos_extensions-e067645736155fb7 --binary /work/.migration-build/target/release/comandos-extensions --cases /work/crates/comandos-extensions/tests/output_schema_traversal_cases.json --full-cases /work/crates/comandos-extensions/tests/output_schema_preflight_full_cases.json --out /work/.superpowers/sdd/schema-traversal-directed.json
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/scripts/rust-python-oracle /home/someguy/.local/share/comandos/extensions-venv /home/someguy/.cache/comandos/tiktoken python /work/crates/comandos-extensions/tests/output_schema_traversal_audit.py --stage-binary /work/.migration-build/target/debug/deps/comandos_extensions-e067645736155fb7 --binary /work/.migration-build/target/release/comandos-extensions --cases /work/crates/comandos-extensions/tests/output_schema_traversal_cases.json --full-cases /work/crates/comandos-extensions/tests/output_schema_preflight_full_cases.json --full --out /work/.superpowers/sdd/schema-traversal-full.json
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/scripts/rust-python-oracle /home/someguy/.local/share/comandos/extensions-venv /home/someguy/.cache/comandos/tiktoken python /work/crates/comandos-extensions/tests/output_schema_audit.py --binary /work/.migration-build/target/release/comandos-extensions --cases /work/crates/comandos-extensions/tests/output_schema_preflight_full_cases.json --out /work/.superpowers/sdd/schema-traversal-stock-after.json
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/scripts/rust-python-oracle /home/someguy/.local/share/comandos/extensions-venv /home/someguy/.cache/comandos/tiktoken python /work/crates/comandos-extensions/tests/output_schema_preflight_audit.py --binary /work/.migration-build/target/debug/deps/comandos_extensions-e067645736155fb7 --out /work/.superpowers/sdd/schema-traversal-stage-after.json
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/scripts/rust-python-oracle /home/someguy/.local/share/comandos/extensions-venv /home/someguy/.cache/comandos/tiktoken python /work/.superpowers/sdd/schema-traversal-marker-audit.py
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/scripts/rust-python-oracle /home/someguy/.local/share/comandos/extensions-venv /home/someguy/.cache/comandos/tiktoken python /work/.superpowers/sdd/schema-traversal-verify-evidence.py
```

The original oracle capture used the same traversal audit with generate and capture-only flags. Later directed additions store their independently captured oracle directly beside original JSON strings. /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.superpowers/sdd/schema-traversal-run-audits.py discovers the executable and dispatches bounded final audits.

## Provenance and evidence paths

/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-extensions/src/output_schema/traversal/PROVENANCE.json records jsonschema 4.26.0 and referencing 0.37.0 source paths, SHA256 hashes, ported functions, owned Rust destinations and intentional exclusions. Exact keyword inventories accompany those hashes. The frozen manifest also records cached Rust API source, CPython identity-source and SDK boundary hashes.

The new notices are byte-identical to the installed originals:

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-extensions/src/output_schema/traversal/JSONSCHEMA-COPYING matches /home/someguy/.local/share/comandos/extensions-venv/lib/python3.11/site-packages/jsonschema-4.26.0.dist-info/licenses/COPYING; SHA256 4f92a015a13c4d1a040bef018aa13430b4f1bc73b41b16bb846c346766de7439.
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-extensions/src/output_schema/traversal/REFERENCING-COPYING matches /home/someguy/.local/share/comandos/extensions-venv/lib/python3.11/site-packages/referencing-0.37.0.dist-info/licenses/COPYING; SHA256 42dcd63495f87b4eb7c7757afa379bb55a53f94afd7a5f657d9adf57236e515c.

Rust tests verify bundled notice hashes and class inventories. The optional exact oracle independently verifies installed original source/notice hashes and bundled bytes. The URI join/defrag handling uses only derived empty-base identities. No general CPython URI algorithm was added.

The SDK source at /home/someguy/.local/share/comandos/extensions-venv/lib/python3.11/site-packages/mcp/client/session.py was rechecked: ValidationError, SchemaError and Unresolvable map to RuntimeError. Audit rows retain actual Python exception names, Unresolvable inheritance and separate dependency/SDK mapping fields. The existing error aggregation at /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/extension_proxy.py was also rechecked. Full MCP diagnostic parity remains outside this dependency audit.

Primary local deliverables and reproducible evidence:

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-extensions/tests/output_schema_traversal_cases.json
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-extensions/tests/output_schema_traversal_audit.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.superpowers/sdd/schema-traversal-directed.json
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.superpowers/sdd/schema-traversal-full.json
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.superpowers/sdd/schema-traversal-frozen.json
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.superpowers/sdd/schema-traversal-invariance.json
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.superpowers/sdd/schema-traversal-stock-before.json
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.superpowers/sdd/schema-traversal-stock-after.json
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.superpowers/sdd/schema-traversal-stage-after.json
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.superpowers/sdd/schema-traversal-oracle-before.json
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.superpowers/sdd/schema-traversal-selector-oracle.json
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.superpowers/sdd/schema-traversal-marker-audit.json

The frozen manifest verifies every protected baseline input and hashes each owned implementation/fixture file. An extraction check proves the worker module is byte-identical after removing its sole new private declaration. No preflight data, historical fixture, historical result, dependency manifest, lockfile or runner was edited.
