# LocalBridge Community Build v1 — Upstream Diff & Security Review

This document provides a formal audit record of the changes introduced in LocalBridge Community Build v1 relative to the upstream pinned commit of `zephyr7030/LocalBridge`.

---

## 1. Commit and Version Baseline

- **Upstream Project:** [zephyr7030/LocalBridge](https://github.com/zephyr7030/LocalBridge)
- **Upstream Version:** `0.1.5`
- **Upstream Commit:** `c8118e73e96b05ef9090b260837a44788aeafaff`
- **Community Build Version:** `0.1.5-community.1`
- **OpenAI tunnel-client Source:** [github.com/openai/tunnel-client](https://github.com/openai/tunnel-client)
- **OpenAI tunnel-client Commit:** `8d55683eeef80bc5e360d95abf4692454fafc615` (version `0.0.11`)

---

## 2. Community-Only File Additions

The following files and directories are Community Build additions and do not exist in the upstream repository:

1. **Automation & Build Scripts (`scripts/`):**
   - `scripts/build-tunnel-client.ps1`: Builds OpenAI `tunnel-client.exe` from verified Go source.
   - `scripts/verify-runtime.ps1`: Validates SHA256 hashes of all bundled runtime binaries and checks for unapproved executables.
   - `scripts/build-community.ps1`: Unified end-to-end community build orchestrator.
   - `scripts/generate-checksums.ps1`: Generates cryptographic SHA256 checksums (`SHA256SUMS.txt`).

2. **Supply Chain Provenance Records (`provenance/`):**
   - `provenance/toolchain.json`: Machine-readable toolchain constraints and actual environment versions.
   - `provenance/tunnel-client.json`: OpenAI tunnel-client build metadata and compiler flags.
   - `provenance/broker.json`: Privileged broker build target, command, and staging configuration.
   - `provenance/runtime-lock.json`: Cryptographic lockfile for all bundled components (Python, MCP, Toolbox, Tunnel Client).

3. **Release & Audit Metadata:**
   - `COMMUNITY-VERSION`: Pinned version tag `0.1.5-community.1`.
   - `SHA256SUMS.txt`: Cryptographic checksum manifest.
   - `UPSTREAM-DIFF.md`: This audit diff document.
   - `TEST-REPORT.md`: Comprehensive status report of all build, security, and environment gates.
   - `BUILD-PROVENANCE.json`: Aggregated build provenance snapshot.

4. **Community Documentation (`docs/`):**
   - `docs/COMMUNITY-BUILD.md`: Step-by-step building, reproducing, and verification guide.
   - `docs/SECURITY-MODEL.md`: Security architecture and trust boundary specification.
   - `docs/UPSTREAM-SYNC.md`: Upstream synchronization policy and conflict resolution rules.

5. **Continuous Integration (`.github/`):**
   - `.github/workflows/community-build.yml`: Automated CI pipeline on Windows runner.

---

## 3. Security-Sensitive Upstream Files Modified

| File | Upstream Purpose | Community Modification | Security Behavior Change | Weakened Integrity / Auth? |
| :--- | :--- | :--- | :--- | :--- |
| `src-tauri/src/tunnel/bundle.rs` | Verifies SHA256 of `tunnel-client.exe` and `LICENSE` before execution | Updated `TUNNEL_CLIENT_SHA256` constant to match the community-built binary hash | **NONE** | **NO** (fail-closed verification is strictly retained) |
| `runtime-manifest.toml` | Declares bundled runtime assets and hashes | Updated `executable_sha256` and marked `vendoring = "source-built"` | **NONE** | **NO** (accurate reflection of source-built binary) |

### Formal Review of Security Semantics:

```text
Security Behavior Changes: NONE
```

- **UAC / Privileged Broker:** Still requires explicit user authorization; no default elevation; no arbitrary elevated shell capability added.
- **MCP Guard Authentication:** Retains mandatory `Authorization: env:LOCALBRIDGE_MCP_GUARD_BEARER` token verification on 127.0.0.1.
- **Network Surface:** No public listeners created; all bridges remain bound strictly to localhost loopback interfaces.
- **Secret Handling:** All credentials (API keys, tunnel tokens) remain injected strictly via child process environment variables and are never leaked to command line arguments, configuration files, or build artifacts.
- **Bundle Integrity:** `verify_bundle()` continues to fail closed if any runtime file is missing, modified, or corrupted.
