# Clippy test-module placement repair - 2026-09-29

## Audited cloud result for 9c43453

Audit performed on 2026-09-28 for branch codex/fix-ci-validation at
9c4345351bf527222401752198d26884002fe4e0. Both PR runs checked out merge
843d8178f7a998bfdf91c93f7f4cb31c264b1260 against
012adf917b06b8a3866b0d77a564ae481c289b4f.

| Run | Verified result |
| --- | --- |
| [CI push 36424064624](https://github.com/MYD548-svg/LocalBridge-Community/actions/runs/36424064624) | FAIL: Windows runtime-resources compile preflight, test-clippy |
| [CI PR 36424072597](https://github.com/MYD548-svg/LocalBridge-Community/actions/runs/36424072597) | FAIL: same Clippy error |
| [Community PR 36424072539](https://github.com/MYD548-svg/LocalBridge-Community/actions/runs/36424072539) | Linux PASS; Windows FAIL with the same Clippy error |

All three Windows jobs passed the first 11 stages and test-compile within
stage 12. test-clippy rejected items after the test module at
src-tauri/src/control_plane/command_output.rs:132. The existing -D warnings
gate promoted clippy::items_after_test_module to an error (exit 101).
Broker-specific compile preflight and later stages, including authentication
repetitions, Rust behavior tests, standalone Clippy, packaging and final
artifact verification, were NOT_RUN. No verified installer was uploaded.
The shared-output behavior introduced in 9c43453 remains unverified by these
cloud runs; successful test compilation is not test execution.

## Repair

Move the existing test module to the end of command_output.rs without changing
production functions or test bodies. Inspection of the other Rust files touched
in 9c43453 found no corresponding trailing production items after their test
modules. No lint suppression, API change, runtime payload/hash change, workflow
change or test weakening is included.

## Local evidence for this repair

- PASS: installed Rust 1.85 Clippy, standalone command_output.rs including tests,
  --edition=2021 --test --emit=metadata -A dead_code -D warnings. The exact
  items_after_test_module error was reproduced before repair during the prior
  audit; the repaired module passes. The dead_code exception is command-local
  for isolated-module unused items; complete-crate CI still uses -D warnings.
  This is metadata analysis, not a linked executable or full-crate Clippy.
- PASS: existing gate from bundled-integrity through frontend-build (8 stages).
  The generated gate report correctly remains PARTIAL, not full acceptance.
- PASS: 32 Node base/build-contract tests; bundled Python 11/11, including
  .cmd, .bat, PowerShell and NUL each exactly ten times (40/40). Marker, exit
  code, replay and NUL leakage assertions are unchanged. No retry-until-pass.
- PASS: handle regression 262 -> 262, maximum growth 8; fixtures retained.
- PASS: formatting, public release policy, licenses (166 npm / 484 cargo),
  schema44, frontend tests (18 tests / 6 files), TypeScript and Vite build.
- PASS: bundled integrity. Runtime source, manifest, metadata and pinned Rust
  hashes are unchanged; historical installer evidence is not new evidence.
- NOT_RUN locally: full Rust compile/Clippy, Rust behavior/public-interface
  tests, authenticated Tunnel probes, NSIS packaging and clean installation.
  MSVC/Windows SDK remain unavailable and were not installed.

The sandbox initially blocked Node spawning git (EPERM) before validation
started. The same authorized gate then ran outside that restriction and passed.
This environment retry is not a retry of a failed behavior assertion.

## Delivery boundary

Stage only command_output.rs and this report after diff review. Push once to
codex/fix-ci-validation, then compare the remote branch SHA with the local commit.
Do not query new Actions, poll, rerun, monitor, merge or publish. New-commit cloud
acceptance remains UNVERIFIED. Preserve all 19 gates, ten authentication probes,
existing triggers and toolchain versions. Keep unrelated untracked material and
all test fixtures. If push is rejected, retain the local commit without force.

## Historical reports

# Shared public command output delivery - 2026-09-28

## Delivery boundary and verified cloud baseline

This change starts from `fe3c18b011a39f0b6837135cabbb595213c52fbb` on
`codex/fix-ci-validation`. The new change's cloud acceptance is **UNVERIFIED**.
Delivery ends after one push and a remote branch SHA comparison. Do not query,
poll, rerun or monitor new Actions, merge the PR, or publish a release.

The following results were audited before this implementation. PR jobs checked
out merge `ebdeda3239bab8c3f71c8b3f1934cfd8d3739429`, based on
`012adf917b06b8a3866b0d77a564ae481c289b4f`.

| Run | Verified result for fe3c18b |
| --- | --- |
| [CI push 36324843555](https://github.com/MYD548-svg/LocalBridge-Community/actions/runs/36324843555) | PASS: 19 stages, Rust library 413/413, verified installer uploaded |
| [CI PR 36324847625](https://github.com/MYD548-svg/LocalBridge-Community/actions/runs/36324847625) | PASS: 19 stages, Rust library 413/413, verified installer uploaded |
| [Community PR 36324847644](https://github.com/MYD548-svg/LocalBridge-Community/actions/runs/36324847644) | Linux PASS; Windows first 14 stages PASS; Rust library 412 PASS / 1 FAIL |

All three passed the 11 Python regressions and ten authentication probes.
Community failed `full_scripts_share_current_user_authority_independent_of_path_spelling`
at the NUL assertion (old server.rs:8320): scripts emitted their markers, but
`echo hidden>nul && echo hidden-error 1>nul 2>nul && echo LB_SCHEMA42_NUL_OK`
returned completed / exit 0 with zero accumulated output. Later standalone
Clippy, NSIS, package integrity and installer artifacts were NOT_RUN for that
Community job. CI installer existence was checked; independent download,
extraction and clean-machine installation were NOT_RUN.

## Repair and evidence limits

- Move pending output, bounded stderr protocol fragments and sticky truncation /
  incomplete diagnostics into one in-memory owner keyed by ExecutionId. The
  facade, background observer and busy direct control route share that owner.
  Execution records retain their existing persistent format. Output owners are
  evicted with their execution records; the existing 1 MiB pending / 256 KiB
  stderr limits are preserved.
- Register an RAII collection guard before upstream calls, and release it after
  returned bytes and diagnostics are staged. No output mutex is held over I/O.
  Delivery keeps reporting running while another collection is outstanding;
  it does not consume pending bytes until all collections have settled. Kill
  retains its bounded timeout response while a competing collection is pending.
- Deliver incremental output exactly once across routes, including terminal
  replay. Preserve authoritative outcomes when another caller finalizes first.
  The busy route now uses the same stderr filtering and fixed incomplete-output
  warning. Warnings stay in structuredContent.warnings, not command data.
- Background retryable transport failures no longer commit a lost terminal.
  Existing cancellation intent and retryable-control regressions remain intact.
- Retain all marker assertions. NUL failures print the complete envelope and
  accumulated output; test logs identify facade/direct delivery paths.
- Move the existing pure stderr-filter functions into the shared module to obey
  the domain/MCP dependency boundary. This does not change the filtering algorithm.

The original direct-terminal regression was added before implementation and
exposes the old empty-output branch by construction. Rust execution was not
available locally, so no locally observed Rust red/green result is claimed.
The cloud logs do not prove that this specific interleaving caused the NUL
failure; the production gap is independently established by code inspection.

## New local evidence

- PASS: test-base, 32 Node tests, including the bundled Python output suite.
- PASS: Python suite, 11 tests; .cmd, .bat, PowerShell and NUL command cases each
  run exactly ten times (40/40), with marker once, exit 0 and empty terminal
  replay. NUL cases also reject leaked hidden text and pseudo nul files.
- PASS: 24-command retained-output/handle regression, 271 -> 271 handles,
  maximum allowed growth 8. All fixtures remain retained.
- PASS: format, public release policy, license audit (166 npm / 484 cargo),
  schema44 architecture scan, frontend tests (18 tests / 6 files), TS/Vite build.
- PASS: bundled integrity; no runtime payload, manifest, metadata or pinned hash
  changed. Historical installer attestations are not new-change evidence.
- PASS: isolated Rust 1.85 metadata/type check of command_output.rs including
  its three concurrency/filter tests. This emits metadata only, not a linked
  test executable, and is not a full-crate compilation or test execution.
- ADDED / NOT_RUN locally: cross-route facade regression; actual HTTP direct
  control regression forced by holding the facade mutex, covering completed,
  failed, cancelled and timed_out output plus warnings/replay; pending-collector
  kill/poll regression; shared-buffer barrier/channel tests. Existing transport
  outage and cancellation regressions remain required.
- NOT_RUN locally: full Rust compilation, Clippy, Rust test execution, public MCP
  E2E, authenticated Tunnel repetitions, NSIS packaging and installation. MSVC /
  Windows SDK are unavailable and were not installed, per user preference.

An intermediate schema44 check rejected an MCP dependency from the new shared
module; moving the pure filter resolved it without changing the gate. The
standalone bundled rustc distribution lacked std; the already installed Rust
1.85 toolchain completed the isolated metadata check. Neither attempt is
represented as a full Rust PASS.

Validation uses the existing gate: test-base, then format through frontend-build;
selected-stage reports remain PARTIAL. Keep all 19 cloud stages, ten auth probes,
workflow triggers and toolchain versions unchanged. No retry-until-success test
logic, Actions rerun, assertion weakening, credential extraction or bulk deletion
was introduced.

## Earlier reports (historical evidence only)

# Command output settlement repair - 2026-09-27

## Delivery boundary

Implementation starts from `9a8abb2e60d7dac3f4060705624dd177d124222a`
on `codex/fix-ci-validation`. This section supersedes the delivery statuses
below. The repaired commit's cloud result is **UNVERIFIED**.
This delivery ends after one push and a remote branch SHA comparison.
No new Actions query, polling, rerun, monitor, merge, or release is authorized.

## Verified cloud baseline (before this repair)

All three runs below belong to head `9a8abb2e60d7dac3f4060705624dd177d124222a`.
PR runs checked out merge `ff388e70e6d6ecc1a3b786b8341f19f936d4499d`,
whose base is `012adf917b06b8a3866b0d77a564ae481c289b4f`.

| Run | Event/profile | Verified result |
| --- | --- | --- |
| [36313820560](https://github.com/MYD548-svg/LocalBridge-Community/actions/runs/36313820560) | CI push / bundled | PASS: all 19 stages; installer uploaded |
| [36313823351](https://github.com/MYD548-svg/LocalBridge-Community/actions/runs/36313823351) | CI PR / bundled | PASS: all 19 stages; installer uploaded |
| [36313823283](https://github.com/MYD548-svg/LocalBridge-Community/actions/runs/36313823283) | Community PR | Linux upstream PASS; Windows FAIL in rust-test |

Community Windows passed its first 14 stages, including the four compile/lint
preflight checks and all ten authentication probes. The Rust library suite
reported 411 passed and 1 failed:
`mcp::server::tests::full_scripts_share_current_user_authority_independent_of_path_spelling`
at `src/mcp/server.rs:8299`. A PowerShell command reported completed / exit 0,
but its accumulated output did not contain the script marker. The later
standalone Clippy stage, NSIS packaging, package integrity and artifacts stages
were NOT_RUN in that run. Installer existence and cloud gate results were
verified for the two CI runs; independent installer download/extraction was
NOT_RUN in this audit.

## Repair and evidence boundaries

- Serialize active snapshots and archival per session. Each response uses a
  single settled lifecycle observation; it cannot re-poll after copying bytes
  and upgrade an older empty snapshot to terminal.
- Require process exit plus pipe-reader completion before publishing terminal.
  Readers use interruptible nonblocking pipes. A shared monotonic five-second
  drain deadline bounds post-exit collection; expiry freezes already-read bytes
  and marks output incomplete with warnings/truncated. Each drain wait is at
  most 200 ms. Cleanup of pending readers/watchdog is deferred with bounded
  joins; request threads never wait indefinitely on a pipe or reader.
- Cache one retained record and redirect stale active-session snapshots to its
  cursor. Retained records reuse the registry lock, avoiding one retained OS
  synchronization handle per completed command. Registry publication does not
  resurrect an evicted record.
- Install readers/watchdog before exposing a new session to pruning. Keep an
  exited session reachable while output is draining, including the kill path.
- Preserve the fixed incomplete-output warning and truncation flag through the
  Rust public result and terminal replay. Do not forward arbitrary private
  warnings. Keep output-marker assertions, with shell/command/accumulated
  output details and per-poll identity/status/byte-count diagnostics.
- Synchronize the coding-tools payload tree hash in runtime metadata, manifest
  and the Rust pin. The tree hash is
  `9f94420accf554f5227e96b58d49be4eb01acb4f99f35d09a9060651ded7f58f`.
  Other runtime components and historical build/installer attestations remain
  unchanged. Original unchanged byte-pinned source lines retain their bytes.

The snapshot/exit and delayed-reader hazards were reproduced with controlled
processes. They match the cloud symptom; the existing cloud log does not prove
which interleaving caused that particular failure. No assertion was weakened,
no test was skipped, and no retry-until-success behavior was added.

## Local validation of this repair

- PASS: test-base, 32 Node tests including the bundled-Python regression runner.
- PASS: final Python output suite, 11 tests, including deterministic exit and
  delayed stdout/stderr cases, archive/snapshot contention, timeout and kill
  drainage, open inherited-pipe cancellation, and startup publication ordering.
- PASS: real Windows .cmd, .bat and PowerShell scripts, ten runs each. Markers
  appear exactly once, exit code is zero, terminal replay returns no old bytes.
- PASS: 24 additional commands in one Runtime; retained output remains readable
  and process handle count stays 261 -> 261 (existing maximum growth: 8).
- PASS: bundled runtime integrity, source formatting, public-release policy,
  dependency licenses (166 npm / 484 cargo packages), schema44 residue scan.
- PASS: frontend tests (18 tests in 6 files) and TypeScript/Vite build.
- PASS: git diff whitespace check. An intermediate check rejected CRLF on new
  byte-pinned source lines; new lines were corrected, hashes recomputed and the
  format gate passed. An initial new fixture used the public `command` key for
  the private API; it was corrected to `cmd` and the suite passed.
- NOT_RUN locally: Rust test compilation, Clippy, full Rust/runtime regressions,
  authenticated Tunnel repetitions and NSIS packaging. No usable MSVC/Windows
  SDK was found; the user chose not to install them. Formatting and Python
  runtime tests are not represented as Rust compilation or public MCP E2E.
- ADDED / NOT_RUN locally: Rust public-envelope incomplete-output regression.
- NOT_RUN: live ChatGPT, UAC and clean-machine installation.
- UNVERIFIED: cloud results for the new repaired commit. Historical passing
  runs above are evidence only for their original SHA.

Commands used: `node scripts/test/ci-gate.mjs --from test-base --through frontend-build`
(test-base PASS, intermediate formatting issue), then
`node scripts/test/ci-gate.mjs --from format --through frontend-build` (all selected
stages PASS), `node --test scripts/test/runtime-output.test.mjs` (final suite PASS),
and `node scripts/test/runtime-integrity.mjs --bundled-only`.
Selected-stage gate reports are PARTIAL by design. No local all-19-stage PASS
is claimed. Test workspaces and unrelated untracked files are retained.

## Historical reports (superseded; original evidence retained)

# LocalBridge concentrated repair - 2026-09-27

## Current delivery boundary

Implementation baseline: `944001d5014858ae4ea5a30a4da173be7f34afd0`.
Branch: `codex/fix-ci-validation`. The user requests one push after local
checks, then immediate stop after comparing the remote branch SHA. No Actions
query, rerun, artifact download, merge, or release is part of this delivery.
**Cloud results for this delivery: UNVERIFIED.** Historical evidence below
belongs to its named commits and is not acceptance evidence for this change.

## Changes in this delivery

- Preserve the production `!error.retryable` cancellation safeguard.
- Replace the resubmission probe's sleeping occupier with the existing facade
  mutex. Scoped request threads are joined; the lock is released before joining
  a blocked request, including during assertion unwinding.
- The HTTP test helper reports connect/configure/write/read/parse failures,
  I/O error kind and received byte count. The first request must time out while
  reading with zero received bytes; only then is the second sent. The requests
  have identical tool arguments and different JSON-RPC IDs. Each execution
  atomically publishes a UUID marker; exactly two valid markers are required.
  This tests client-response abandonment, not every server OperationTimedOut path.
- Add per-runtime, cfg(test)-only one-shot control faults. ConnectionUnavailable
  and McpExited exercise the actual facade mappings without killing the process.
  Tests cover preserved cancellation intent/session/pending output, same-session
  recovery, real cancellation, terminal replay and non-retryable unknown sessions.
  Injected McpExited is an error-mapping probe, not proof of supervisor recovery
  after actual process exit. Production pending_runtime_fault handling is intact;
  the independent real-process-stop regression remains.
- Retain test workspaces, including Drop cleanup and release-preflight fixtures.
  Missing-path/identity tests preserve their assertions by retaining displaced
  directories. The production filesystem delete implementation is unchanged.
- Keep all 19 gate stages, ten authentication probes and existing workflows.
  Only the shared runtime-resources call opts into --compile-preflight. After
  toolbox/bootstrap and BUILDING evidence, test compilation and Clippy run for
  default all-targets and the feature-gated broker, before release broker build.
  The feature is not passed to Tauri packaging. COMPILE-PREFLIGHT.json records
  checkout SHA, exact commands, statuses and exit codes; failed checks leave
  later checks NOT_RUN and broker evidence BUILDING. Final gate hashes include
  this report. Ordinary resource preparation does not repeat the preflight.

## Validation status

- PASS: 31 Node base/gate/build-regression tests in the initial local run,
  including all four injected preflight failures, release failure and success.
  Final check results are recorded in the delivery message.
- Rust test compilation, Clippy, runtime regressions and three repeated timing
  runs: NOT_RUN locally. No usable MSVC/Windows SDK was found. No toolchain was
  installed and formatting is not being represented as compilation.
- New dynamic resubmission/recovery tests: ADDED, AWAITING WINDOWS EXECUTION.
- Current-head Actions and installer/artifact verification: UNVERIFIED by user
  instruction. Live ChatGPT, UAC and clean-machine installation: NOT_RUN.

## Historical report (prior commits only; not revalidated this delivery)

# LocalBridge CI repair verification - rewritten 2026-09-26

This report was rewritten from scratch in explicit UTF-8 (ASCII subset): the
previously committed file was byte-corrupted (124 U+FFFD replacement sequences,
invalid UTF-8 - the damage was real data loss, not a terminal rendering
artifact). Facts below were recovered from the code, the CI logs and the
downloaded artifacts. The running commit chain is deliberately not repeated
here; history lives in the commit log, current state lives here.

## Status

Branch `codex/fix-ci-validation`, PR #1 -> `main` in `MYD548-svg/LocalBridge-Community`.
The sole acceptance gate is `node scripts/test/ci-gate.mjs` (19 stages); the
community workflow runs the same gate with `LOCALBRIDGE_BUILD_PROFILE=community`.

Head `46e0afb` (the repair chain through the installer-content verification,
the retryable-transient terminalization fix and the full-7-Zip installer
check) completed all three runs with success:

| Workflow | Event | Run | Result |
| --- | --- | --- | --- |
| CI | push | 36248206192 | success (19/19 stages PASS) |
| CI | pull_request | 36248208546 | success (19/19 stages PASS) |
| LocalBridge Community Build | pull_request | 36248208631 | success (19/19 stages PASS; Linux upstream-tests job PASS) |

Earlier accepted heads (each with its own three green runs recorded in the
history below): `2f3de89` (push 36235265911 / PR 36235269939 / community
36235269870), `050193d`, and the intermediate review heads. PR-event runs
checkout the merge ref; each PR run's merge commit was verified via the API
to have exactly base `012adf9` + head as parents. Branch protection on
`main`: not readable (404) and no applicable rulesets via the rules API -
recorded as unverified rather than claimed absent.

## Cancel-intent vs retryable-transport-failure fix (current cycle)

Production defect fixed in `CodingToolsRuntimeAdapter::control_command`
(`src-tauri/src/mcp/facade.rs`): the command_control error path treated
"an error with code SessionUnavailable/RuntimeUnavailable plus a recorded
cancellation intent" as a completed cancellation and persisted a durable
`Cancelled` terminal - even when the error was a RETRYABLE transport
failure (connection refused, HTTP status, health timeout; those answer
with `retryable: true` via `normalize_runtime_error`). Cancellation intent,
cancellation delivery and process termination are different facts: a
transport outage leaves the process state unknown, so the fix gates the
cancelled-finalization on `!error.retryable`. Upstream-answered
"session is gone" errors (via `normalize_private_error`, which answers
with `retryable: false`) keep the deliberate accepted-cancellation-wins
contract; retryable outages now keep the intent recorded, the execution
Running and the public session controllable. The later error branch (added
in `02a6a4f`) already kept retryable errors non-terminal; the earlier
cancelled-ification branch had bypassed it - this cycle closes that gap.

Regression coverage (goes through the real error-handling paths):

- `kill_with_retryable_transport_failure_keeps_the_execution_and_cancel_intent`
  (facade-level, real bundled runtime): a detached command is started, the
  upstream process is really stopped, and the kill then fails inside
  `private_call_with_timeout`. Asserts the retryable error envelope (no
  fabricated cancelled success), the execution still Running, the recorded
  "KILL" intent intact, no durable terminal, and the same for a poll issued
  while the intent is pending.
- `kill_intent_survives_a_transport_outage_until_the_runtime_reports_the_real_terminal`
  (control-plane level, deterministic mock): a transport outage
  (`RuntimeCommandControlError::Unavailable`, the PEP mapping for
  connection failures) must not terminalize; after recovery the next real
  observation resolves the terminal (Cancelled via the recorded intent) with
  the observed output carried; replays stay stable without re-appending
  output; a late kill on the terminal execution surfaces the contract error
  and does not overwrite the recorded terminal.
- The JS terminal driver keeps polling envelopes flagged `retryable: true`
  and surfaces non-retryable ones immediately (unknown or unowned sessions
  answer with `retryable: false`, so they are never retried forever).

Dynamic duplicate-execution boundary (previously documented analytically,
now covered by an added fault-injection test; successful execution remains unverified): the new
`resubmission_after_an_abandoned_submission_leaves_two_executions` test
holds the facade execution lane with a foreground command, lets a
submission's client read budget expire (the caller never receives any
session identity - the degraded-runner submit-timeout situation), then
resubmits the same command and observes with counted workspace-local side
effects that BOTH runs execute: the abandoned submission is still
processed by the PEP and its command runs (orphan liveness), and the
resubmission produces a second execution. A first cut of this probe
assumed the healthy upstream would also leave queued submissions
unacknowledged; the cloud run disproved that (queued submissions are
acknowledged in ~50ms), so the stall is now injected at the facade lane,
which is where the historical degraded-runner timeouts actually stalled.
This pins the documented risk: resubmission remains restricted to the
reviewed side-effect-free test scenarios, and production submit
idempotency would require a protocol-level change (idempotency keys or
durable submission records) that is out of scope for this cycle.

After this review cycle the branch gained additional commits; the final
acceptance object is the new head's own three runs. Their run IDs and artifact
verification results are recorded in the delivery message rather than by
re-committing this document again.

## Review verdicts on the earlier timing/retry changes

### d3ba9eb - widened promptness bounds (1.5-5 s -> 10-20 s)

Reviewed test by test, looking for whether the widened bounds alone could mask
the guarded failure. Verdicts:

- `command_control_kill_is_not_blocked_by_unrelated_foreground_work` - still
  discriminative. The 20 s bounds are hang guards; the discriminating
  assertions are the outcomes: the poll must observe `status: "running"` (a
  poll serialized behind the 10 s foreground work would return after the
  detached command had already terminalized), the kill must still observe a
  live session, and the foreground work must end `ProcessCancelled`, not
  completed. All three fail if control requests queue behind foreground work.
- `task_control_cancel_owns_detached_public_command_session`,
  `edit_task_control_cancel_reaches_running_filesystem_hash` - still
  discriminative: they assert the joined call terminates
  `isError: true` / `ProcessCancelled` with the projection converging to Idle,
  so a naturally-completing command (inside the widened bound) still fails.
- `cancellation_reaches_actual_upstream_while_tool_call_is_running` - **was
  weakened** and is fixed in this cycle: its command completes naturally at
  ~10 s, which now fits inside the 20 s settle bound, and the test only
  asserted "some JSON-RPC response arrived". It now also asserts
  `isError: true`, error code `ProcessCancelled` and terminal
  `status: "cancelled"` - a cancellation that never reaches the upstream
  produces a success envelope and fails the test regardless of timing.

### 5ce9004 - blanket `SessionUnavailable` retry in poll helper

Traced to the production call chain. The comment's claimed race is real, but
the blanket test-side retry masked it instead of fixing it: in
`control_command_during_work` (`control_plane/command_control.rs`) the
poll-observation path mapped `ExecutionRegistryError::AlreadyTerminal`
(concurrent finalizer won the `finish` race - the upstream facade's
`normalize_command_result` finalizes the shared file-backed registry on
terminal observations) to `ExecutionConflict`, surfaced as
`SessionUnavailable` with the "terminal-state conflict" message - exactly
the 9a27919 failure signature. Fixed in production this cycle: when a poll
loses the finish race it now replays the durable terminal of the same
execution, carrying the observed incremental output (the first cut of this
fix replayed an output-less envelope and broke the schema27/r1
output-accumulating assertions on `37659cc` - caught by the cloud run and
corrected); it never fabricates or overrides a terminal. The test-side
tolerance was **removed** so genuinely unavailable or unknown sessions
surface as contract errors again. A deterministic regression test
(`poll_returns_the_durable_terminal_when_a_concurrent_call_won_the_finish_race`)
reproduces the race with a concurrent-finalizer runtime mock and asserts the
observed output survives the replay.

### 071b5bf / 70340ec - resubmission after submission-budget timeout

Traced semantics: a submit that returns `OperationTimedOut` without session
identity does **not** prove the command was not executed. The facade's private
call timeout (`private_call_with_timeout`) does not cancel the upstream
request; the upstream may run the queued/started command later, and the facade
terminalizes the public session with the error. Every resubmission therefore
risks a second execution. The resubmitted scenario commands are all
observation-only (`Write-Output`, `Start-Sleep`, `cd`), so no persistent side
effect can be doubled, and the workflow retry's failed attempt terminalizes its
own checkpoint. Made explicit and scoped this cycle: the helper is renamed to
`submit_side_effect_free_public_command` with the safety condition documented,
and both inline retry loops carry the same note. A dynamic fixture proving the
duplicate-execution count empirically (inject timeout, count executions,
inspect orphan liveness) requires a Rust toolchain - NOT_RUN locally, see
below; the production-side orphan execution behavior itself is unchanged by
this cycle and remains a known hazard for non-idempotent client commands.

### 163033f / 2f3de89 - broker attestation and installer content

The staged-broker-vs-evidence check and the config-mapping guard are sound and
retained; the build-tree copy in `target/release` is deliberately not compared
to the staged evidence (the build-regression fixture proves that a differing
build-tree copy does not fail the gate). Negative scenarios now covered by
tests: tampered staged broker -> `SHA256 mismatch`; evidence not PASS ->
`broker build incomplete`; evidence missing/empty -> `missing`; emptied
`bundle.resources` -> `attested staged broker` guard.

But the config pointing at the staged file does not prove what the installer
carries. Physical verification was performed on all three `2f3de89` installers
(extracted offline with 7-Zip, no installation executed): **each installer
declared `localbridge-privileged-broker.exe` twice** - the attested staged
binary (882 688 bytes) plus a 904 704-byte app-feature-set copy that tauri's
NSIS template emits from the cargo binary list (`get_binaries` returns every
`[[bin]]` target; its `{{#each binaries}}` loop runs after the resources loop
and overwrites the resource at install time). The installed broker was
therefore never the attested staged binary, and the same class of behavior hit
the app binary (tauri patches it with bundle-type information at packaging).
This is fixed in production this cycle: the broker `[[bin]]` is gated behind a
`privileged-broker` feature (`required-features`), so the bundler's binary list
no longer includes it while `prepare-lb018-resources.mjs` builds it with that
feature explicitly; the installer now carries only the attested staged broker.
The gate itself now opens any existing NSIS installer during
`runtime-integrity` (package-integrity timing on CI): it rejects duplicate
entry names (a later same-name entry would silently overwrite an attested
file) and verifies the carried broker's SHA256 against the staged evidence.
The new census was validated against all three known-broken `2f3de89`
installers - all three are rejected - and the carried-broker hash comparison
was validated by full offline extraction of those installers (79/79 runtime
payload files matched their attested hashes; only the tauri-patched app binary
and the overwritten broker differed).

Provenance and manifests distinguish build-tree artifacts
(`target/release/...`), staged-to-package artifacts (`target/release-stage/...`)
and the final installer (`target/release/bundle/nsis/...`), and the
bundled/community evidence is kept separate by profile
(`bundled-verified-installer` / `ci-diagnostics` vs
`community-verified-installer` / `community-diagnostics`,
`BUILD-PROVENANCE.json` `profile` field).

## Cycle regressions and pre-existing transients exposed by the review runs

The first two review-push rounds surfaced four distinct issues; each was
diagnosed from its job log before any retry:

1. `37659cc` (all three runs, 407/408): the first cut of the finish-race
   replay answered with an output-less envelope, breaking the
   output-accumulating assertions in schema27 and r1 session-B. Fixed in
   `050193d`: the replay carries the observed stdout/stderr/exit_code with
   the durable outcome; the concurrent-finalizer regression test asserts the
   output survives.
2. `050193d` CI pull_request: rust-test PASSED (the replay fix is effective),
   but my new installer check failed at `installer listing failed (2)` - the
   toolbox `7z.exe` is 7za from `7z2602-extra.7z`, which cannot parse NSIS
   archives. Fixed: the gate resolves a full 7-Zip first (preinstalled on CI
   runner images), with the toolbox copy as last resort.
3. `050193d` CI push: schema28's session-less `command_control read` is
   served through the facade execution guard; when a work request still held
   it, the single-shot read received the contract's retryable
   `RuntimeUnavailable` ("control plane busy") and failed. Fixed test-side:
   both session-less reads retry only `retryable: true` envelopes within a
   bounded 30 s deadline.
4. `050193d` Community pull_request: revision46's stream-probe polls observed
   a durable `Lost` terminal - the production command_control error path
   terminalized the execution on ANY non-OperationTimedOut error, including
   retryable transport/control-lane transients (retryable errors carry
   `retryable: true` by contract). Fixed in production: retryable errors now
   keep the Execution non-terminal and the kill intent intact, exactly like
   OperationTimedOut; non-retryable errors still terminalize. The JS
   terminal driver classifies retryable error envelopes as pending for the
   same reason, keyed on the envelope's contract flag rather than blanket
   code matching, so genuinely unavailable sessions still surface.

## Original-snapshot contrast (verdict and limits, unchanged)

Run 36218847261 (import snapshot `5ea0e25` + one workflow-only helper commit)
failed at rust-test with the same failure family as the task branch's early
heads: `schema27_public_facade_runtime_semantics_are_real_end_to_end`
(`ProcessTimedOut` family), `task_control_cancel_owns_detached_public_command_session`
(submission returned without session identity), and
`actual_tunnel_discovery_sends_the_authenticated_pep_header` (probe race).
This proves the failure family reproduces in the imported code on this
infrastructure; it does **not** prove that every later failure had the same
root cause, that all slowness is runner degradation (its inner cause - why a
spawned PowerShell produces no output for ~30 s - is not observable from job
logs and remains stated as uncertainty), or that the two runs had identical
load. The contrast run is not repeated; no new evidence requires it.

## Ignored tests

None. No `#[ignore]` attributes exist in the Rust suite and every cloud log in
this cycle reports `0 ignored`; the `chatgpt_black_box` cargo target and the
JS black-box scenarios execute on the runners. A cargo-test success therefore
means all compiled tests ran.

## Cloud evidence history (billing blocker kept separate)

| Head | Runs (push / PR / community) | Outcome |
| --- | --- | --- |
| 9add3d6 | 35456808403 / 35456809857 / 35456809851 | rust-test FAIL (revision46 assertion; helper panics on bounded timeouts; one real degraded 30 s timeout) |
| abae096 | 36218255642 / 36218256983 | auth-repeat probe race FAIL |
| 6ea6a84 | 36218961677 / 36218963630 / 36218963629 | auth-repeat probe race FAIL |
| de26450 | 36221409446 / 36221412164 / 36221412184 | auth-repeat PASS; one transient schema28 submission timeout |
| d64a1c7 | 36223868229 / 36223870698 / 36223870794 | one 5 s cancellation-settle exceedance (bound since widened with outcome assertions restored) |
| 3089d5f | 36226819061 / 36226822441 / 36226822436 | lib suite 407/407; revision46 `Write-Error` scenario hit 30 s default |
| 9a27919 | 36228890501 / 36228893464 / 36228893382 | terminal-state finish race (fixed in production this cycle) |
| fae42e7 | 36231334954 / 36231336494 / 36231336476 | rust-test + clippy + NSIS reached; CI failed package-integrity; Community job never started - GitHub billing blocker "recent account payments have failed or your spending limit needs to be increased", both attempts, zero steps executed. User-side account issue, resolved separately by the user; distinct from any code or gate behavior. |
| 2f3de89 | 36235265911 / 36235269939 / 36235269870 | all three success; artifacts downloaded and physically verified (see broker section) |

## Local checks executed for this cycle

| Check | Result |
| --- | --- |
| `node --test scripts/test/build-regression.test.mjs` | PASS - 8 tests, including the new negative broker-evidence cases and the installer-census unit test |
| `node --check` on every changed `.mjs` | PASS |
| Rust format check (vendored rustfmt 1.85.0, edition 2024) on changed `.rs` files | PASS |
| Encoding audit of all tracked text files | TEST-REPORT.md was the only corrupted file; rewritten |

## NOT_RUN (must not be claimed as passed)

- Local execution of Rust tests, clippy, NSIS packaging and the full gate: no
  MSVC/Windows SDK locally; these establish acceptance only in cloud CI.
- Dynamic duplicate-execution fixture (injected submission timeout with
  countable side effects, orphan liveness, workflow re-application counts):
  needs a Rust toolchain; the production orphan-execution hazard is documented
  from code, not yet demonstrated by a fixture.
- Live ChatGPT connection, interactive UAC elevation, and a clean Windows
  installation of the produced installer: outside this phase; recorded
  NOT_RUN in `TEST-REPORT.json` `environmentAcceptance`.
- The inner cause of the degraded-runner stalls remains unexplained; fixes
  rest on measured degradation, the timeout-clock investigation and the
  contrast run.
- Residual exposure unchanged from the previous cycle: the JS
  `settleAcceptedPublicCommand` throws on a submission-level
  `OperationTimedOut`, and `revision46.mjs` workflow prepare/edit/verify has no
  retry (a failed attempt terminalizes the workflow; the edit patch is not
  re-applicable without workspace cleanup). Both are designed against real
  evidence only if a run fails there.
