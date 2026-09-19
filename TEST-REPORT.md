# LocalBridge CI repair verification — 2026-09-20

## Status

**CLOUD ACCEPTANCE IN PROGRESS — LOCAL NON-LINK CHECKS VERIFIED; LINK-TYPE STAGES CLOUD-ONLY.**

Branch: `codex/fix-ci-validation`, PR #1 → `main` in `MYD548-svg/LocalBridge-Community`. Commit chain for this repair cycle: `2786aac` (gate unification, Linux Go prerequisite, probe rewrite, Guard regression, auth-repeat) → `1ca0c35` (source-check realpath normalization for 8.3 runner temp) → `f0269f1` (fixture canonicalization) → `708c6d9` (probe mirrors production Guard streamable-HTTP contract) → `ea1f745` (rustfmt canonical form) → `0423e26` (probe concurrency fix + report) → this commit (probe idle-connection handling + widened test windows).

## Cloud evidence on `ea1f745` and `0423e26` (all runs FAIL at auth-repeat)

| Head | Run | Workflow | Event | First failure |
| --- | --- | --- | --- | --- |
| `ea1f745` | `35446550872` | CI | push | `auth-1` FAIL (exit 101) |
| `ea1f745` | `35446552469` | CI | pull_request | `auth-1` FAIL after 478.0s (exit 101) |
| `ea1f745` | `35446552470` | LocalBridge Community Build | pull_request | `auth-1` PASS (345.6s), `auth-2` FAIL after 57.6s (exit 101) |
| `0423e26` | `35454187616` | CI | push | `auth-1` FAIL after 459.1s (exit 101) |
| `0423e26` | `35454190068` | CI | pull_request | `auth-1` FAIL (exit 101) |
| `0423e26` | `35454190097` | LocalBridge Community Build | pull_request | `auth-1` FAIL after 483.8s (exit 101) |

All preceding gate stages passed in every run, including `format`, `runtime-resources` (859.9s, broker build) and `staged-integrity`. `rust-test`, `rust-clippy`, `nsis-package`, `package-integrity` and `artifacts` have not executed on any head since the stage order places them after `auth-repeat`.

### Evidence-backed root cause (two layers)

**Layer 1 — serial probe serving (`ea1f745`).** The failing assertion reports `no authenticated MCP initialization before deadline`. The real client's startup traffic is concurrent: the OAuth WWW-Authenticate probe (POST then GET on the MCP URL, 1s deadline), the RFC 9728 discovery GETs and the MCP initialize POST (2s deadline) share one keep-alive HTTP client (`pkg/oauth/module.go`, `pkg/mcpclient/fxmodule.go` of the pinned client). The probe's test double served connections serially; concurrent connections stalled past those deadlines and the client abandoned the initialize. The intermittent signature (Community run: `auth-1` passed, `auth-2` failed) matches a race, not a product defect: the authorized `POST /mcp` observation confirms the production authentication path itself.

**Layer 2 — unsolicited 404 on idle probe connections (`0423e26`).** After the concurrency fix, all three runs still failed, and the observation sequence shifted to exactly: two connections with **no request bytes at all**, `GET /mcp` (authorized), the two RFC 9728 discovery GETs, and still no initialize. Reading the pinned client and its Go SDK (`go-sdk v1.4.1`) shows:

- the client dial-and-write path means an observation with zero request bytes is a connection the transport dialed but had not yet written its request to (or an abandoned attempt);
- the double answered every silent connection after 1s with an **unsolicited 404 and close** — something a real HTTP server never does;
- `connectStartupProbe` (`pkg/mcpclient/fxmodule.go`) wraps such a transport failure as `ErrRejected` and the client **never retries the initialize**.

A request written onto one of those connections in the closing window is therefore swallowed unobserved and unretried. A local reproduction with the same bundled `tunnel-client.exe`, the same flags and a byte-compatible double that (like a real server) leaves silent connections alone reached `mcp session initialized` in **14 ms** — confirming the client and the double's response contract are compatible and the failure is the idle-connection handling plus runner startup variance.

### Fix in this commit (test code only, no production change)

1. `serve_probe_connection` no longer responds to connections that have not produced a request: it keeps waiting on an idle connection (up to a 60s idle deadline) and closes silently on peer-close or idle timeout — mirroring real server behavior and removing the request-swallowing race. All Guard contract responses are unchanged (`708c6d9`).
2. Every accepted connection is served on its own thread (kept from `0423e26`).
3. The observation window is widened from 10s to 30s and the probe server lifetime from 15s to 60s to absorb CI runner startup variance; no assertion was weakened or removed.

## Executed local checks (2026-09-19/20, worktree including this fix)

| Check | Actual result |
| --- | --- |
| Local gate `node scripts/test/ci-gate.mjs --through frontend-build` (toolchains, dependencies, tunnel-source, bundled-integrity, test-base, format, public-release, licenses, schema44, frontend-test, frontend-build) | **PASS — 11 stages** |
| Rust format check (rustfmt 1.85.0, `--edition 2024 --check --config skip_children=true`) | PASS — re-run on this fix, `PRE_RELEASE_FORMAT_CHECK=PASS rust_files=1` |
| Client-side compatibility reproduction: bundled `tunnel-client.exe` + same flags/env against a byte-compatible probe double | PASS — `mcp session initialized` (server `localbridge-ci-probe`) within 14 ms of the probe hook; OAuth discovery failure observed as non-blocking, matching production where the Guard serves no `/.well-known` metadata |
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
