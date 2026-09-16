# LocalBridge CI repair verification — 2026-09-16

## Status

**LOCAL CHECKS PARTIALLY VERIFIED — NOT CI ACCEPTED / NOT RELEASE READY.**

Base commit: `012adf917b06b8a3866b0d77a564ae481c289b4f`. Repairs are uncommitted local changes. No remote Actions were triggered, no commits were pushed, and no Release was published.

The earlier report's "Pre-release Ready" and "no BUILD_GATE failed" conclusions are withdrawn. Historical Actions on 2026-09-06 failed: community run `34019155194` lacked toolbox/broker resources; ordinary CI run `34019155218` passed 403 Rust unit tests but failed its real-Tunnel authentication assertion. Earlier run `34017921799` failed upstream Go tests on Windows.

## Executed local checks

| Check | Actual result |
| --- | --- |
| Node regression / infrastructure / black-box client / preflight tests | PASS — 21 tests |
| Existing frontend unit tests | PASS — 18 tests |
| TypeScript and Vite production build | PASS |
| Schema44 architecture residue scan | PASS |
| PowerShell entrypoint syntax | PASS |
| Changed Rust file formatting, rustfmt 1.85.0 | PASS — syntax/format only, not compilation |
| Bundled runtime integrity | PASS — Python and Coding Runtime trees, Tunnel and metadata pins |
| Toolbox preparation, repeated execution | PASS — pinned files reused without directory deletion |
| Independent real-Tunnel MCP probe with dummy credentials | PASS — 10 sequential runs; MCP initialization and observed headers matched |
| Complete runtime integrity without fresh broker evidence | Correctly FAILED — missing `broker-build.json`; no false PASS |
| Shared gate toolchain preflight | FAILED — `spawnSync rustc ENOENT`; later stages recorded NOT_RUN |

Node regressions include exact source selection with stale sibling directories, rejection of mismatched/dirty checkouts, Tunnel-only hash changes, missing/empty/corrupt files, staged toolbox mapping, unexpected executables, broker bootstrap/rebuild/failure and stage fail-fast behavior. Test fixture files are retained for manual cleanup.

The independent Node probe is diagnostic evidence for the bundled binary. It does **not** replace execution of the changed Rust authentication test, nor establish the cause of the historical CI failure. That test now captures multiple requests, distinguishes routes, completes MCP initialization and reports redacted facts. A new Guard regression checks missing/wrong/correct Bearer handling.

## Not executed

- Changed Rust tests, ten repetitions through the Rust harness, complete Rust suite and Clippy: a configured Rust/Cargo/MSVC build environment is unavailable locally. A downloaded standalone formatter was used only for formatting.
- Source-built Tunnel and complete upstream Go suite: Go is unavailable locally; Linux suite is now a prerequisite of the community Windows workflow.
- Fresh Broker build, final staged integrity success, NSIS build and final artifact checksums.
- Repaired ordinary/community Actions on the same commit: not authorized to push or trigger.
- Live ChatGPT connection, interactive UAC and clean Windows installation: outside this phase.

## Acceptance and evidence

The sole gate is `node scripts/test/ci-gate.mjs`; community builds use `scripts/build-community.ps1`. Successful local partial stages must not be reported as a full gate PASS. Per-run reports live in `tests/artifacts/ci/`; successful full runs also produce current provenance and checksums. The root `SHA256SUMS.txt` records baseline repository inputs only and is not an installer checksum manifest.

Both repaired workflows must pass for the same commit before CI acceptance. NSIS success establishes packaging only. See `docs/COMMUNITY-BUILD.md` for prerequisites, stage selection and failure handling.
