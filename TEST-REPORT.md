# LocalBridge CI repair verification — 2026-09-26

## Status

**CLOUD ACCEPTANCE IN PROGRESS — LOCAL NON-LINK CHECKS VERIFIED; LINK-TYPE STAGES CLOUD-ONLY.**

Branch: `codex/fix-ci-validation`, PR #1 → `main` in `MYD548-svg/LocalBridge-Community`. Commit chain for this repair cycle: `9add3d6` (probe idle-connection handling, prior cycle) → `ca81c21` (revision46 queue blocker settled through the accepted-command terminal driver) → `4a5872f` (Rust test support treats bounded command-control timeouts as pending) → `abae096` (schema27 scenario commands carry explicit degraded-runner budgets) → `af8bb74` (`wait_for_output` satisfies when the marker arrives with the terminal response) → this report commit.

## Cloud evidence on `9add3d6` (all three expected runs FAIL at rust-test)

| Head | Run | Workflow | Event | Result |
| --- | --- | --- | --- | --- |
| `9add3d6` | `35456808403` | CI | push | rust-test FAIL: `revision46_reported_failures_are_rechecked_through_the_external_client` |
| `9add3d6` | `35456809857` | CI | pull_request | rust-test FAIL: same revision46 assertion |
| `9add3d6` | `35456809851` | LocalBridge Community Build | pull_request | rust-test FAIL: 402 passed / 4 failed (lib); Linux upstream-tests job PASS |

All stages before `rust-test` passed in every run, including `auth-repeat` on both profiles. `rust-clippy`, `nsis-package`, `package-integrity` and `artifacts` were not reached.

### Failure taxonomy (evidence-backed, from the three job logs)

1. **revision46 first-response assertion (both CI runs).** `tests/black-box/chatgpt/revision46.mjs:304` asserted `assertSuccess(await blocker).status === "completed"` on the first response of a `yield_time_ms: 10_000` command. The cloud returned the documented non-terminal `running` (the PowerShell start plus 4 s of work did not fit the yield window on that runner). The scenario already had `settleAcceptedPublicCommand` available and used it in six other places.
2. **Rust test helpers treated a bounded timeout as terminal (Community run, 3 of 4).** `settle_public_command` (test_support.rs:602) and `DetachedCommand::wait_for_output` (test_support.rs:509) panicked on poll responses whose `structuredContent.data` is null. The production contract (facade.rs:2135 keeps the Execution non-terminal on `OperationTimedOut` for command_control actions; remediation: "poll later to observe the same Execution") makes that response a keep-polling signal: the cloud payloads read `code: OperationTimedOut, phase: transport, retryable: true, data: null`. The helpers' poll budgets (`wait_ms` 25/100/1000) expired whenever the private runtime answered slowly.
3. **A real process timeout (Community run, 1 of 4).** `schema27_public_facade_runtime_semantics_are_real_end_to_end` observed `ProcessTimedOut` after 30 702 ms with zero output for a command that completes in ~1 s when healthy. `run_bounded_command` starts the timeout clock at `ResumeThread`, so queue/accept time is excluded and the PowerShell process itself ran ~30 s producing nothing. The same degraded window is measured inside the run: a trivial `Write-Output` command needed 21 poll rounds (~20 s+) before admission. Attribution: runner degradation (cold process start / AV / load), not a production-logic defect — the 30 s default `timeout_ms` was inherited by omission, while sibling scenarios in the same file already pass explicit `timeout_ms: 120000`.

### Fixes (test code only; no production change; no assertion weakened)

1. `ca81c21` — the revision46 blocker is settled through the existing `settleAcceptedPublicCommand` (stable `session_id` from the initial response, `running`/`OperationTimedOut` both non-terminal, one absolute 60 s deadline), still requires `completed`, now also verifies the expected `LB_QUEUE_BLOCKER_DONE` stdout via the output handle, and keeps the queued-request cancellation and no-file-side-effect assertions. The blocker's `timeout_ms` was raised 20 000 → 60 000 with the measured degraded-start evidence; nothing asserts a timeout outcome. `command_lifecycle.test.mjs` gained the "already terminal first response settles without polling" case (existing four cases retained).
2. `4a5872f` — a pure `CommandPollObservation` classifier (`Running` / `BoundedWaitExpired` / `Terminal` / `Invalid`) derived from the facade response contract is now shared by `settle_public_command`, `DetachedCommand::wait_for_output` and `poll_public_command_to_terminal`. `OperationTimedOut` continues polling the same stable session without resetting the absolute deadline; terminal failures and typed errors still report immediately; a timeout before any session identity was delivered panics explicitly. Unit-tested against payload shapes taken verbatim from the cloud logs. The now-unused `DetachedCommand::status()` was removed.
3. `abae096` — the schema27 scenario command and its settle-fed siblings (`quoted`, `powershell_error`, `cmd_cd_switch`, `auto_utf8`, `autoload`) declare explicit `timeout_ms: 120000`, matching the file's existing convention (baseline, r1, schema28_detached); the schema27 poll loop treats a bounded timeout as pending and its convergence deadline is 150 s. Production timeout semantics are unchanged — no timeout was raised in production code, and `ProcessTimedOut` remains a terminal failure everywhere.
4. `af8bb74` — `wait_for_output` treats "marker observed in the terminal response" as satisfied instead of panicking, per the observe-then-classify contract.

## Original-snapshot contrast (goal §5)

- **A (original):** import snapshot `5ea0e25` on branch `codex/upstream-contrast-5ea0e25`; **B (current):** this head. The failing test code is byte-identical between A and B (verified by Git object comparison in the previous cycle for `test_support.rs`, `server.rs` modulo the auth regression test, and `tests/black-box/chatgpt/`).
- **Method:** one helper commit on the contrast branch adds `.github/workflows/upstream-contrast.yml` (run condition only — no business, test, or assertion change) which runs the same gate stages `node scripts/test/ci-gate.mjs --through rust-test` on `windows-latest` with the same toolchain versions as the task branch's gate. Build outputs are isolated by construction (fresh runner, fresh checkout).
- **Status:** the contrast branch push was blocked at write time by intermittent `github.com:443` connection resets from the local network (the task-branch pushes of the same window went through after retries); the contrast runs and their verdict are recorded in the delivery message. Until that verdict exists, dynamic attribution to the original snapshot remains open; the static identity argument above is the evidence of record. Classification will follow the goal §5.4 table (both fail with the same signature ⇒ original-code reproduction in this environment; original passes ⇒ timing/load/profile sensitivity; original cannot run ⇒ blocked, recorded as such).

## Executed local checks (2026-09-26, worktree at this head)

| Check | Actual result |
| --- | --- |
| `node --test tests/black-box/chatgpt/command_lifecycle.test.mjs` | PASS — 5 tests (4 retained + 1 new) |
| Rust format check (vendored rustfmt 1.85.0, `--edition 2024 --check --config skip_children=true`) on both changed Rust files | PASS |
| cargo check (type-level, no linking) | NOT_RUN — fails at dependency build scripts requiring `link.exe`; MSVC/Windows SDK intentionally not installed locally |
| Rust unit/black-box test execution locally | NOT_RUN — no MSVC; covered by the cloud gate |

## Not executed (NOT_RUN — must not be claimed as passed)

- Local `rust-test`, `auth-repeat`, `rust-clippy`, `nsis-package`, `package-integrity`, `artifacts`: no MSVC/Windows SDK; these establish acceptance only in cloud CI.
- Live ChatGPT connection, interactive UAC, clean Windows installation: outside this phase (recorded in TEST-REPORT.json `environmentAcceptance`).
- The `schema27` ProcessTimedOut inner cause (why the degraded process produced no output) is not directly observable from the job log; the fix rests on the measured degradation and the semantics investigation above. Residual uncertainty is stated rather than resolved.

## Acceptance and evidence

The sole gate is `node scripts/test/ci-gate.mjs` (19 stages); community builds use `LOCALBRIDGE_BUILD_PROFILE=community`. Successful local partial stages are not a full gate PASS. Final run IDs and links are recorded in the delivery message rather than by re-committing this document; the three expected runs on this head are the acceptance object, and artifact verification (TEST-REPORT.json PASS, provenance, SHA256 manifest, installer checksum match, bundled/community separation) is performed against those runs.
