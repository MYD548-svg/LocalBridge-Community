# LocalBridge CI repair verification — 2026-09-20

## Status

**CLOUD ACCEPTANCE IN PROGRESS — LOCAL NON-LINK CHECKS VERIFIED; LINK-TYPE STAGES CLOUD-ONLY.**

Branch: `codex/fix-ci-validation`, PR #1 → `main` in `MYD548-svg/LocalBridge-Community`. Commit chain for this repair cycle: `2786aac` (gate unification, Linux Go prerequisite, probe rewrite, Guard regression, auth-repeat) → `1ca0c35` (source-check realpath normalization for 8.3 runner temp) → `f0269f1` (fixture canonicalization) → `708c6d9` (probe mirrors production Guard streamable-HTTP contract) → `ea1f745` (rustfmt canonical form) → this commit (probe concurrency fix + this report).

## Cloud evidence on `ea1f745` (three runs, all FAIL at auth-repeat)

| Run | Workflow | Event | First failure |
| --- | --- | --- | --- |
| `35446550872` | CI | push | `auth-1` FAIL (exit 101) |
| `35446552469` | CI | pull_request | `auth-1` FAIL after 478.0s (exit 101) |
| `35446552470` | LocalBridge Community Build | pull_request | `auth-1` PASS (345.6s), `auth-2` FAIL after 57.6s (exit 101) |

All preceding gate stages passed in all three runs, including `format` (so the `ea1f745` formatting fix is confirmed), `runtime-resources` (859.9s, broker build), and `staged-integrity`. `rust-test`, `rust-clippy`, `nsis-package`, `package-integrity` and `artifacts` have not executed on any head since the stage order places them after `auth-repeat`.

### Evidence-backed root cause

The failing assertion reports `no authenticated MCP initialization before deadline` with this redacted observation sequence (run `35446552469`):

- one connection observed with an empty request,
- `POST /mcp` **with valid authorization** but `initialized: false`,
- a second empty connection,
- `GET /.well-known/oauth-protected-resource/mcp` and `GET /.well-known/oauth-protected-resource`, both authorized,
- no initialize request ever observed.

The real client's startup traffic is concurrent: the OAuth WWW-Authenticate probe (POST then GET on the MCP URL, 1s deadline), the RFC 9728 discovery GETs and the MCP initialize POST (2s deadline) share one keep-alive HTTP client. The probe's test double served connections serially, one request per accept; concurrent connections stalled past those deadlines and the client abandoned the initialize. The intermittent signature (Community run: `auth-1` passed, `auth-2` failed) matches a race, not a deterministic product defect: the production authentication header behavior itself was confirmed correct by the authorized `POST /mcp` observation.

### Fix in this commit (test code only, no production change)

`src-tauri/src/tunnel/runtime.rs` probe now handles every accepted connection on its own thread (`serve_probe_connection`), preserving the Guard contract assertions introduced in `708c6d9` (sessionless GET → 400 `session_id_required`, `Mcp-Session-Id` on initialize/tools-list responses, protocol-version echo, 401 on bad/missing Bearer, 404 off-route).

## Executed local checks (2026-09-19, worktree including this fix)

| Check | Actual result |
| --- | --- |
| Local gate `node scripts/test/ci-gate.mjs --through frontend-build` (toolchains, dependencies, tunnel-source, bundled-integrity, test-base, format, public-release, licenses, schema44, frontend-test, frontend-build) | **PASS — 11 stages** |
| Rust format check (rustfmt 1.85.0, `--edition 2024 --check --config skip_children=true`) | PASS — included in gate run above |
| Pinned Tunnel source build with Go 1.26.2 (commit `8d55683`, `-SkipTests`, binary not registered) | PASS — `TUNNEL_BUILD=PASS`, version `0.0.11+8d55683` |
| Bundled runtime integrity | PASS — included in gate run above |

Toolchain inventory: Node 24.16, git 2.46, Rust 1.85.0 (rustup-managed), Go 1.26.2. MSVC/Windows SDK is intentionally not installed locally, therefore compile-and-link checks cannot run locally.

## Not executed (NOT_RUN — must not be claimed as passed)

- `rust-test`, `auth-repeat` execution, `rust-clippy`, `nsis-package`, `package-integrity`, `artifacts` locally: no MSVC/Windows SDK; these run only in cloud CI, which also establishes the acceptance evidence.
- Complete upstream Go test suite: runs in the Community workflow's Linux prerequisite job (passed on `ea1f745`'s run; re-run triggers on the new head).
- Live ChatGPT connection, interactive UAC and clean Windows installation: outside this phase.
- Cloud acceptance for this head: pending; the three expected runs on this commit are the acceptance object.

## Acceptance and evidence

The sole gate is `node scripts/test/ci-gate.mjs` (19 stages); community builds use `scripts/build-community.ps1` (`LOCALBRIDGE_BUILD_PROFILE=community`). Successful local partial stages are not a full gate PASS. Final run IDs and links are recorded in the delivery message rather than by re-committing this document; if this report is updated after pushing, the new head must re-qualify through the complete cloud acceptance (three runs, same head, recorded base).
