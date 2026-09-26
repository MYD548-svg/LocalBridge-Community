# LocalBridge CI repair verification — 2026-09-26

## Status

**CLOUD ACCEPTANCE IN PROGRESS — LOCAL NON-LINK CHECKS VERIFIED; LINK-TYPE STAGES CLOUD-ONLY.**

Branch: `codex/fix-ci-validation`, PR #1 → `main` in `MYD548-svg/LocalBridge-Community`. Commit chain for this repair cycle: `9add3d6` (probe idle-connection handling, prior cycle) → `ca81c21` (revision46 queue blocker settled through the accepted-command terminal driver) → `4a5872f` (Rust test support treats bounded command-control timeouts as pending) → `abae096` (schema27 scenario commands carry explicit degraded-runner budgets) → `af8bb74` (`wait_for_output` satisfies when the marker arrives with the terminal response) → `6ea6a84` (this report, first revision) → `bf54898` (probe double serves repeated requests on one connection like the production Guard) → `071b5bf` (scenario commands resubmit when their submission wait budget expired) → this report revision.

## Original-snapshot contrast verdict (goal §5, decided 2026-09-26)

Run `36218847261` ("Upstream Contrast", branch `codex/upstream-contrast-5ea0e25` = import snapshot `5ea0e25` plus one workflow-only helper commit) executed `node scripts/test/ci-gate.mjs --through rust-test` on `windows-latest` with the same toolchain versions as the task branch. **The untouched original FAILED at rust-test with the same failure family:**

| Original-snapshot failure | Signature |
| --- | --- |
| `schema27_public_facade_runtime_semantics_are_real_end_to_end` | `assertion left == right failed` at server.rs:6340, `state: "failed"` — the same assert that failed in the 9add3d6 Community run (ProcessTimedOut family) |
| `task_control_cancel_owns_detached_public_command_session` | panicked `"second detached public session"` — a submission whose initial response carried no session identity (transport budget expired) |
| `actual_tunnel_discovery_sends_the_authenticated_pep_header` | panicked at the ORIGINAL pre-fix probe code (runtime.rs:770): `real tunnel binary omitted the authenticated MCP header` |

Per the goal §5.4 table this is the "both versions fail with matching signatures" outcome: **the rust-test failures are present in the imported code and reproduce in this environment; they are not introduced by the task branch.** The contrast limitation is documented below (helper workflow commit, stage list of the 5ea0e25 gate).

## Cloud evidence on the fix heads `abae096` / `6ea6a84` (docs-only difference between them)

| Head | Run | Workflow | Event | Result |
| --- | --- | --- | --- | --- |
| `abae096` | `36218255642` | CI | push | auth-repeat FAIL: `auth-1..auth-6` PASS, `auth-7` probe race (runtime.rs:998, "no authenticated MCP initialization") |
| `abae096` | `36218256983` | LocalBridge Community Build | pull_request | FAIL (auth-repeat stage) |
| `6ea6a84` | `36218961677` / `36218963630` / `36218963629` | CI push / CI PR / Community PR | all | auth-repeat FAIL on the first probe round, same signature as `abae096` `auth-7` |

The mcp fixes themselves were not reached by these runs (auth-repeat precedes rust-test in the 19-stage gate). The probe failure is a residual test-double race, not a product defect: the observation sequence shows the client's full authenticated startup traffic (OAuth POST probe, session DELETE, sessionless GET, both RFC 9728 discovery GETs) and **no initialize at all** — the initialize POST was written onto a connection the one-shot double had already closed. Every double response carried `Connection: close`, so the shared keep-alive client opens a fresh connection per request in the clean case (verified by the local reproduction log), but a request written in the closing window of a served connection is swallowed and never retried (`connectStartupProbe` wraps the transport failure as `ErrRejected`). The same swallow existed in the original snapshot's probe (contrast run above).

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
5. `bf54898` — the probe test double now serves repeated requests on one connection (keep-alive loop, no `Connection: close`), mirroring the production Guard's connection lifecycle; per-request response contracts are unchanged. This removes the closing-window swallow of the initialize POST described above. Idle-connection behavior (wait, never answer) is retained from `9add3d6`.
6. `071b5bf` — detached scenario submissions that expire their submission wait budget (facade budget = `yield_time_ms` + 3 s; the response then carries no session identity and the attempt is terminal) are resubmitted with a bounded 60 s deadline in the test support layer: `submit_public_command` for the cancel-ownership test's two submissions and schema27, a retry loop in both r1 session-B threads, and the same tolerance inside `start_detached_command`. The scheduler-readiness waits around those submissions are 3 s → 10 s. Scenario assertions (ownership, isolation, cancellation, output) are unchanged; a genuinely failing submission still panics with the response dump.

## Original-snapshot contrast (goal §5)

- **A (original):** import snapshot `5ea0e25` on branch `codex/upstream-contrast-5ea0e25`; **B (current):** the task branch. The failing test code is byte-identical between A and B (verified by Git object comparison in the previous cycle for `test_support.rs`, `server.rs` modulo the auth regression test, and `tests/black-box/chatgpt/`).
- **Method / helper changes:** one helper commit on the contrast branch adds `.github/workflows/upstream-contrast.yml` (run condition only — no business, test, or assertion change) which runs `node scripts/test/ci-gate.mjs --through rust-test` on `windows-latest` with the same toolchain versions as the task branch's gate. The 5ea0e25 gate's stage list (test-base → rust-test, 9 stages) is the snapshot's own; the current gate adds stages before rust-test (tunnel-source, bundled-integrity, staged-integrity, auth-repeat), so the contrast has slightly less preceding load. Build outputs are isolated by construction (fresh runner, fresh checkout). The verdict from this run is recorded at the top of this report.

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
- The `schema27` ProcessTimedOut inner cause (why the degraded process produced no output) is not directly observable from the job log; the fix rests on the measured degradation, the timeout-clock investigation, and the original-snapshot contrast above. Residual uncertainty is stated rather than resolved.
- The submission-timeout and probe-race frequencies on the runners are measured from single runs (probe race: `auth-7` of 10 rounds on `abae096`; submission timeout: once in the original-snapshot contrast); they are environmental and vary run to run.

## Acceptance and evidence

The sole gate is `node scripts/test/ci-gate.mjs` (19 stages); community builds use `LOCALBRIDGE_BUILD_PROFILE=community`. Successful local partial stages are not a full gate PASS. Final run IDs and links are recorded in the delivery message rather than by re-committing this document; the three expected runs on this head are the acceptance object, and artifact verification (TEST-REPORT.json PASS, provenance, SHA256 manifest, installer checksum match, bundled/community separation) is performed against those runs.
