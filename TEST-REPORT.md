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
now verified by fault injection): the new
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
