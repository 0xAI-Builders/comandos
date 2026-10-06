# T4 runtime — private launch and configuration application

2026-10-05. Follows functional inventory b81f5d2 and bounded metadata cache cdbec83; consumes approved ExtensionStore b9b7ee1 (local cherry-pick b5ba752).

## Result and API

Completed extension_launch::prepare_launch(registry:&Value,harness:&str,account:&str,cwd:&Path,selection:&Value,runtime_dir:&Path,operation_id:&str,paths:&capabilities::Paths)->Result<Value,LaunchError>. Ports operation-id validation, internal inventory and canonical choice validation, private bundle/artifact creation, hashes, full TOML overrides, synthetic shared proxies, all five supported adapters and the manifest contract. Codex retains preexisting skill overrides; Claude strict excludes selected-off servers and refuses active plugin MCP loss, expansions and relative cwd; OpenCode produces a compact selected overlay. Grok contributes each existing config and leader socket; AGY materializes native/project MCP and exclusion config or copies selected skill resources into private directory mounts.

SessionConfiguration Kind::Extensions now implements target/identity/conversation/account guards, revision and draft ownership validation, canonical selections, exact transcript checks, native configuration command, preserved model flags, bundle/environment wrapping, unchanged selection handling and continuity. Removed the hardcoded prepare decline. OpenCode snapshot environment revalidation and origin recovery wrapping consume native inventory instead of declining. Normal model/account changes map the verified extension choices by stable ID or unique name/scope/plugin, reject loss of an active extension and create a private destination bundle. Snapshot, process pinning, extension verification and rollback retain existing exactly-once controller behavior.

The pre-claim probe validates read-only plans and mappings without materializing launch/environment artifacts or snapshot transcripts. A stale draft produces a Python-compatible callback error after claim, rather than declining native ownership. No public placeholder bundle is returned. Inventory releases its metadata Mutex immediately after construction/counting, before MCP work.

## Safety and deliberate corrections

Only private directories/files are created (0700/0600, exclusive creation, fsync); shared configuration is read and mounted/copied, never rewritten. Namespace preflight is a native fixed child proof using unshare/mount/setpriv, explicit positional paths, cleared environment and original HOME. It checks child bind visibility, unchanged parent content and zero execution capabilities, has a ten-second deadline and terminates/reaps its own process group. Missing/forbidden namespace support returns the same pre-stop error as Python. No production Python subprocess is added. Existing wrap_command emits the established cc-extension-session command; replacing that launcher remains O5 in the migration plan.

Fixed a clear legacy symlink bug: checking runtime_dir only after resolve loses the link and can chmod/write its external target. Native preparation rejects the supplied symlink before resolve; regression RED confirmed the old behavior wrote a bundle into the target. GREEN preserves target mode0755 and empty contents. Resource copying has a 10000-entry budget and cycle detection; uncertainty fails before stopping an agent. All tests use temporary homes, synthetic credentials, private -S tmux with /dev/null config and compiled fake Codex. No real agent, SSH, user tmux session, service, browser or personal config was touched.

## Evidence

RED: missing prepare_launch API; extension adapter probe declined; parent HTTP positive apply returned503 versus Python202. Final native suites: extension_inventory_oracle4/4; extension_prepare_oracle3/3; session_configuration_oracle14/14; session_profiles_oracle7/7 (28 total). Bundle/manifest/artifact hashes for Codex/Claude/OpenCode compare serialized Python results after only private-directory normalization. Grok/AGY private mount contents and namespace rejection compare Python while shared files remain byte-identical. This host does not permit the namespace proof, so actual successful namespace execution is not claimed.

Private application regression confirms a new owned fake-agent PID, confirmed state, loaded demo=false, replay without relaunch, unchanged shared config, and a subsequent normal effort change retaining selection. Preparation regression confirms shell launch and revision errors and that probe creates no extension-launches directory. Existing model/account/recovery/stuck-origin/foreign-PID/wrapper tests all remain green. Final all-target runtime clippy with warnings denied, fmt and diff checks pass.

## Coordination and remaining gates

Root owns HTTP extensions/configure/recovery integration and its positive apply/recover twins. Independent review remains pending. Reviewer owns the separately reported tokenizer FIFO/nonregular read fix; this tranche does not duplicate it. Browser/UI verification remains Mac-only and is outside these native backend tests.
