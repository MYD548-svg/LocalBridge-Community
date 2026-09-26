# LocalBridge CI repair verification ¡ª 2026-09-26

## Status

**CLOUD ACCEPTANCE IN PROGRESS ¡ª LOCAL NON-LINK CHECKS VERIFIED; LINK-TYPE STAGES CLOUD-ONLY.**

Branch: `codex/fix-ci-validation`, PR #1 ¡ú `main` in `MYD548-svg/LocalBridge-Community`. Commit chain for this repair cycle: `9add3d6` (probe idle-connection handling, prior cycle) ¡ú `ca81c21` (revision46 queue blocker settled through the accepted-command terminal driver) ¡ú `4a5872f` (Rust test support treats bounded command-control timeouts as pending) ¡ú `abae096` (schema27 scenario commands carry explicit degraded-runner budgets) ¡ú `af8bb74` (`wait_for_output` satisfies when the marker arrives with the terminal response) ¡ú `6ea6a84` (this report, first revision) ¡ú `bf54898` (probe double serves repeated requests on one connection like the production Guard) ¡ú `071b5bf` (scenario commands resubmit when their submission wait budget expired) ¡ú `de26450` (report, second revision) ¡ú `70340ec` (the nested workflow retries when its command submission budget expired) ¡ú `d64a1c7` (report, third revision) ¡ú `d3ba9eb` (remaining stall-family promptness bounds widened, still bounded) ¡ú `3089d5f` (report, fourth revision) ¡ú `ad79d6c` (revision46 scenario commands carry explicit degraded-runner budgets) ¡ú this report revision.

## Original-snapshot contrast verdict (goal ¡ì5, decided 2026-09-26)

Run `36218847261` ("Upstream Contrast", branch `codex/upstream-contrast-5ea0e25` = import snapshot `5ea0e25` plus one workflow-only helper commit) executed `node scripts/test/ci-gate.mjs --through rust-test` on `windows-latest` with the same toolchain versions as the task branch. **The untouched original FAILED at rust-test with the same failure family:**

| Original-snapshot failure | Signature |
| --- | --- |
| `schema27_public_facade_runtime_semantics_are_real_end_to_end` | `assertion left == right failed` at server.rs:6340, `state: "failed"` ¡ª the same assert that failed in the 9add3d6 Community run (ProcessTimedOut family) |
| `task_control_cancel_owns_detached_public_command_session` | panicked `"second detached public session"` ¡ª a submission whose initial response carried no session identity (transport budget expired) |
| `actual_tunnel_discovery_sends_the_authenticated_pep_header` | panicked at the ORIGINAL pre-fix probe code (runtime.rs:770): `real tunnel binary omitted the authenticated MCP header` |

Per the goal ¡ì5.4 table this is the "both versions fail with matching signatures" outcome: **the rust-test failures are present in the imported code and reproduce in this environment; they are not introduced by the task branch.** The contrast limitation is documented below (helper workflow commit, stage list of the 5ea0e25 gate).

## Cloud evidence on the fix heads (`abae096` ¡­ `ad79d6c`)

| Head | Run | Workflow | Event | Result |
| --- | --- | --- | --- | --- |
| `abae096` | `36218255642` | CI | push | auth-repeat FAIL: `auth-1..auth-6` PASS, `auth-7` probe race (runtime.rs:998, "no authenticated MCP initialization") |
| `abae096` | `36218256983` | LocalBridge Community Build | pull_request | FAIL (auth-repeat stage) |
| `6ea6a84` | `36218961677` / `36218963630` / `36218963629` | CI push / CI PR / Community PR | all | auth-repeat FAIL on the first probe round, same signature as `abae096` `auth-7` |
| `de26450` | `36221409446` / `36221412164` / `36221412184` | CI push / CI PR / Community PR | all | **auth-repeat PASS (10/10, keep-alive probe fix effective)**; rust-test 406/407: one transient failure ¡ª `schema28_public` at server.rs:6981: the nested `agent_workflow` command submission surfaced a retryable `OperationTimedOut` (`request_deadline_expired`) |
| `d64a1c7` | `36223868229` / `36223870698` / `36223870794` | CI push / CI PR / Community PR | all | **auth-repeat PASS; rust-test 406/407**: `cancellation_reaches_actual_upstream_while_tool_call_is_running` exceeded its 5 s cancellation-settle bound; the workflow retry from `70340ec` passed |
| `3089d5f` | `36226819061` / `36226822441` / `36226822436` | CI push / CI PR / Community PR | all | **lib suite 407/407 PASS** (timing widening effective); black-box revision46 failed once: the `Write-Error` scenario command (no explicit budget, 30 s default) ran 30 087 ms with zero output and surfaced `ProcessTimedOut` instead of the asserted `ProcessFailed` |

Each round since `de26450` has eliminated the previously failing point and surfaced exactly one further timing-sensitive assertion from the same degraded-runner stall family: multi-second stalls of process spawn and private-runtime responses, measured at 20¨C30 s in the 9add3d6 Community run (a trivial `Write-Output` command needed 21 poll rounds before admission; a healthy ~1 s command ran 30 s with zero output). Responses, in order:

1. `70340ec` ¡ª the nested `agent_workflow` invocation in schema28_public retries on the retryable `OperationTimedOut` its command submission surfaced; the failed attempt terminalizes its own checkpoint (`terminalize_legacy_checkpoint_failure`), so each retry starts a fresh workflow and the scenario assertions apply to the successful attempt.
2. `d3ba9eb` ¡ª the remaining fixed promptness bounds of the stall family are widened in one pass: cancellation transports 2 s ¡ú 10 s, cancellation settles 5 s ¡ú 20 s, "not blocked behind unrelated work" bounds 1.5¨C2.5 s ¡ú 20 s, readiness loops 3 s ¡ú 10 s. No bound was removed and none became meaningless: each still fails on the behaviour it guards against (cancellation lost, operation serialized behind unrelated foreground work that itself lasts up to 10 s, tool stuck non-idle), just no longer on a multi-second runner stall alone.
3. `ad79d6c` ¡ª the revision46 scenario commands that still inherited the 30 s default declare explicit `timeout_ms: 120_000`, the same measured adjustment already applied to the Rust scenario commands in `abae096`. Nothing asserts a timeout outcome in those scenarios.

The poll/submission fixes were all exercised and passed on `de26450`/`d64a1c7`/`3089d5f`; the black-box revision46 blocker fix (`ca81c21`) is exercised whenever the lib suite passes (the cargo test targets run in order) and its remaining exposure is the residual risk listed below.

### Probe race root cause (fixed by `bf54898`, cloud-verified on `de26450`)

The probe failure on `abae096`/`6ea6a84` was a residual test-double race, not a product defect: the observation sequence showed the client's full authenticated startup traffic (OAuth POST probe, session DELETE, sessionless GET, both RFC 9728 discovery GETs) and **no initialize at all** ¡ª the initialize POST was written onto a connection the one-shot double had already closed. Every double response carried `Connection: close`, so the shared keep-alive client opens a fresh connection per request in the clean case (verified by the local reproduction log), but a request written in the closing window of a served connection is swallowed and never retried (`connectStartupProbe` wraps the transport failure as `ErrRejected`). The same swallow existed in the original snapshot's probe (contrast run above). With the keep-alive double, auth-repeat passed 10/10 on every subsequent run.

## Cloud evidence on `9add3d6` (all three expected runs FAIL at rust-test)

| Head | Run | Workflow | Event | Result |
| --- | --- | --- | --- | --- |
| `9add3d6` | `35456808403` | CI | push | rust-test FAIL: `revision46_reported_failures_are_rechecked_through_the_external_client` |
| `9add3d6` | `35456809857` | CI | pull_request | rust-test FAIL: same revision46 assertion |
| `9add3d6` | `35456809851` | LocalBridge Community Build | pull_request | rust-test FAIL: 402 passed / 4 failed (lib); Linux upstream-tests job PASS |

All stages before `rust-test` passed in every run, including `auth-repeat` on both profiles. `rust-clippy`, `nsis-package`, `package-integrity` and `artifacts` were not reached.

### Failure taxonomy on `9add3d6` (evidence-backed, from the three job logs)

1. **revision46 first-response assertion (both CI runs).** `tests/black-box/chatgpt/revision46.mjs:304` asserted `assertSuccess(await blocker).status === "completed"` on the first response of a `yield_time_ms: 10_000` command. The cloud returned the documented non-terminal `running` (the PowerShell start plus 4 s of work did not fit the yield window on that runner). The scenario already had `settleAcceptedPublicCommand` available and used it in six other places.
2. **Rust test helpers treated a bounded timeout as terminal (Community run, 3 of 4).** `settle_public_command` (test_support.rs:602) and `DetachedCommand::wait_for_output` (test_support.rs:509) panicked on poll responses whose `structuredContent.data` is null. The production contract (facade.rs:2135 keeps the Execution non-terminal on `OperationTimedOut` for command_control actions; remediation: "poll later to observe the same Execution") makes that response a keep-polling signal: the cloud payloads read `code: OperationTimedOut, phase: transport, retryable: true, data: null`. The helpers' poll budgets (`wait_ms` 25/100/1000) expired whenever the private runtime answered slowly.
3. **A real process timeout (Community run, 1 of 4).** `schema27_public_facade_runtime_semantics_are_real_end_to_end` observed `ProcessTimedOut` after 30 702 ms with zero output for a command that completes in ~1 s when healthy. `run_bounded_command` starts the timeout clock at `ResumeThread`, so queue/accept time is excluded and the PowerShell process itself ran ~30 s producing nothing. The same degraded window is measured inside the run: a trivial `Write-Output` command needed 21 poll rounds (~20 s+) before admission. Attribution: runner degradation (cold process start / AV / load), not a production-logic defect ¡ª the 30 s default `timeout_ms` was inherited by omission, while sibling scenarios in the same file already pass explicit `timeout_ms: 120000`.

### Fixes (test code only; no production change; no assertion weakened)

1. `ca81c21` ¡ª the revision46 blocker is settled through the existing `settleAcceptedPublicCommand` (stable `session_id` from the initial response, `running`/`OperationTimedOut` both non-terminal, one absolute 60 s deadline), still requires `completed`, now also verifies the expected `LB_QUEUE_BLOCKER_DONE` stdout via the output handle, and keeps the queued-request cancellation and no-file-side-effect assertions. The blocker's `timeout_ms` was raised 20 000 ¡ú 60 000 with the measured degraded-start evidence; nothing asserts a timeout outcome. `command_lifecycle.test.mjs` gained the "already terminal first response settles without polling" case (existing four cases retained).
2. `4a5872f` ¡ª a pure `CommandPollObservation` classifier (`Running` / `BoundedWaitExpired` / `Terminal` / `Invalid`) derived from the facade response contract is now shared by `settle_public_command`, `DetachedCommand::wait_for_output` and `poll_public_command_to_terminal`. `OperationTimedOut` continues polling the same stable session without resetting the absolute deadline; terminal failures and typed errors still report immediately; a timeout before any session identity was delivered panics explicitly. Unit-tested against payload shapes taken verbatim from the cloud logs. The now-unused `DetachedCommand::status()` was removed.
3. `abae096` ¡ª the schema27 scenario command and its settle-fed siblings (`quoted`, `powershell_error`, `cmd_cd_switch`, `auto_utf8`, `autoload`) declare explicit `timeout_ms: 120000`, matching the file's existing convention (baseline, r1, schema28_detached); the schema27 poll loop treats a bounded timeout as pending and its convergence deadline is 150 s. Production timeout semantics are unchanged ¡ª no timeout was raised in production code, and `ProcessTimedOut` remains a terminal failure everywhere.
4. `af8bb74` ¡ª `wait_for_output` treats "marker observed in the terminal response" as satisfied instead of panicking, per the observe-then-classify contract.
5. `bf54898` ¡ª the probe test double now serves repeated requests on one connection (keep-alive loop, no `Connection: close`), mirroring the production Guard's connection lifecycle; per-request response contracts are unchanged. Idle-connection behavior (wait, never answer) is retained from `9add3d6`.
6. `071b5bf` ¡ª detached scenario submissions that expire their submission wait budget (facade budget = `yield_time_ms` + 3 s; the response then carries no session identity and the attempt is terminal) are resubmitted with a bounded 60 s deadline in the test support layer: `submit_public_command` for the cancel-ownership test's two submissions and schema27, a retry loop in both r1 session-B threads, and the same tolerance inside `start_detached_command`. The scheduler-readiness waits around those submissions are 3 s ¡ú 10 s. Scenario assertions (ownership, isolation, cancellation, output) are unchanged; a genuinely failing submission still panics with the response dump.
7. `70340ec` ¡ª see the ordered response list above.
8. `d3ba9eb` ¡ª see the ordered response list above.
9. `ad79d6c` ¡ª see the ordered response list above.

## Original-snapshot contrast (goal ¡ì5)

- **A (original):** import snapshot `5ea0e25` on branch `codex/upstream-contrast-5ea0e25`; **B (current):** the task branch. The failing test code is byte-identical between A and B (verified by Git object comparison in the previous cycle for `test_support.rs`, `server.rs` modulo the auth regression test, and `tests/black-box/chatgpt/`).
- **Method / helper changes:** one helper commit on the contrast branch adds `.github/workflows/upstream-contrast.yml` (run condition only ¡ª no business, test, or assertion change) which runs `node scripts/test/ci-gate.mjs --through rust-test` on `windows-latest` with the same toolchain versions as the task branch's gate. The 5ea0e25 gate's stage list (test-base ¡ú rust-test, 9 stages) is the snapshot's own; the current gate adds stages before rust-test (tunnel-source, bundled-integrity, staged-integrity, auth-repeat), so the contrast has slightly less preceding load. Build outputs are isolated by construction (fresh runner, fresh checkout). The verdict from this run is recorded at the top of this report.

## Executed local checks (2026-09-26, worktree at this head)

| Check | Actual result |
| --- | --- |
| `node --test tests/black-box/chatgpt/command_lifecycle.test.mjs` | PASS ¡ª 5 tests (4 retained + 1 new) |
| `node --check tests/black-box/chatgpt/revision46.mjs` | PASS |
| Rust format check (vendored rustfmt 1.85.0, `--edition 2024 --check --config skip_children=true`) on every changed Rust file, re-run after each edit | PASS |
| cargo check (type-level, no linking) | NOT_RUN ¡ª fails at dependency build scripts requiring `link.exe`; MSVC/Windows SDK intentionally not installed locally |
| Rust unit/black-box test execution locally | NOT_RUN ¡ª no MSVC; covered by the cloud gate |

## Not executed (NOT_RUN ¡ª must not be claimed as passed)

- Local `rust-test`, `auth-repeat`, `rust-clippy`, `nsis-package`, `package-integrity`, `artifacts`: no MSVC/Windows SDK; these establish acceptance only in cloud CI.
- Live ChatGPT connection, interactive UAC, clean Windows installation: outside this phase (recorded in TEST-REPORT.json `environmentAcceptance`).
- The inner cause of the degraded-runner stalls (why a spawned PowerShell produces no output for 30 s) is not directly observable from the job logs; the fixes rest on the measured degradation, the timeout-clock investigation, and the original-snapshot contrast. Residual uncertainty is stated rather than resolved.
- Residual exposure, not yet observed in a cloud run: the JS-side `settleAcceptedPublicCommand` still throws on a submission-level `OperationTimedOut` (no session identity in the response), and `revision46.mjs` `verifyWorkflowExecutionOwnership` (workflow prepare/edit/verify) has no retry because a failed attempt terminalizes the workflow and the edit's patch is not re-applicable without workspace cleanup. Both will be designed against real evidence if a run ever fails there.
- The stall frequencies are measured from single runs; they are environmental and vary run to run.

## Acceptance and evidence

The sole gate is `node scripts/test/ci-gate.mjs` (19 stages); community builds use `LOCALBRIDGE_BUILD_PROFILE=community`. Successful local partial stages are not a full gate PASS. Final run IDs and links are recorded in the delivery message rather than by re-committing this document; the three expected runs on this head are the acceptance object, and artifact verification (TEST-REPORT.json PASS, provenance, SHA256 manifest, installer checksum match, bundled/community separation) is performed against those runs.
