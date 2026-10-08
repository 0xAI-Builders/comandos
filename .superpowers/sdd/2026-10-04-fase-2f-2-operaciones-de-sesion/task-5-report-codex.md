# T5 — native session profiles and capabilities

2026-10-05. Base 7d5aabd; implementation in migration/rust-fase2f-ops-tail. No deployment or personal runtime changes.

## Result

Completed native capabilities/configuration discovery, plugin discovery and origins, session-profile inventory, launch draft and launch arguments, MCP descriptions, YAML skill metadata, and ProfilesGet/ProfilesPost/ProfileApply adapters. Persistence remains in comandos-store; runtime depends on store without an inverse dependency. Production does not invoke Python. Existing route selection now exposes its shared resolver to profile apply, retaining its original contracts.

Independently reviewed CRUD commit 8521e77 against lib/session_profiles.py and its oracle tests: no findings. Save/update preserves creation time, validation and transaction behavior; delete and malformed rows retain established behavior.

## Coverage and contracts

Capabilities ports provider-home/config selection, JSON/JSONC/TOML readers, project/parent/user layers, plugin roots/manifests/scope/version selection, MCP merge and declarations, configuration errors and limitations, and session capabilities. Profile runtime ports skill roots/discovery/frontmatter/shadowing/policy, inventory, launch capabilities, launch draft, Codex overrides and Claude private MCP configuration. MCP description catalog preserves configuration descriptions, catalog fallback and Unicode casefold matching. All nine harness inventory paths are covered by differential tests.

GET response preserves Python field insertion order as well as values. POST retains save/delete response and error contracts. Apply validates the profile, draft, account/model route and dry-run launch arguments before returning availability. Existing uncertain-input fallback remains side-effect free. Three parity fixture rows extend the established operations fixture.

Native TOML implements the Python 3.10 public-field projection and Python datetime/arbitrary-number representation. YAML uses a native YAML 1.1 constructor (including aliases, merges, explicit tags and duplicate keys), avoiding YAML 1.2 boolean divergence. Skill prefix reads normalize universal newlines before counting 16000 characters; both CR-only frontmatter and CRLF boundary cases match Python.

## Safety and limits

Config reads require regular files, open nonblocking and cap reads at 2 MiB. Non-UTF8 paths or uncertain inputs decline before mutation. Discovery retains bounded traversal. YAML construction adds a 256-depth/50000-node budget to prevent alias expansion abuse; exhausted input produces no frontmatter metadata. Claude real-launch output uses a unique private directory (0700) and file (0600), without changing shared files; dry runs create nothing. Regression tests use temporary homes, synthetic credentials and fake agent binaries. No personal tmux socket, agent, SSH credential, browser, service or configuration was used.

## Verification

RED captured missing native APIs and legacy HTTP fallback; differential RED cases also exposed YAML 1.1 scalar behavior, TOML projection/datetime/numeric representation, and CR-only/CRLF prefix semantics. The serialized HTTP comparison exposed an inventory/capabilities insertion-order mismatch; corrected before commit and retained as a regression.

Final focused suite: runtime session_profiles_oracle 7/7, server dash_native_ops_profiles 2/2, store session_profiles_oracle 3/3. Server twin compares normalized serialized objects, preserving key order while normalizing temporary paths/timestamps/hash IDs. Existing configuration/operations regression tests previously passed 9/9; runtime/server library units previously passed 178/178. Final fmt check and diff check pass; final runtime/server all-target clippy with warnings denied recorded at commit closure.

## Remaining work

T4 extension-specific inventory/metadata, prepare_launch and configuration application are separate authorized work, not claimed by T5. Independent parent review of this implementation remains pending. Browser verification is confined to the Mac and was unnecessary for these native contracts.
