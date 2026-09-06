# LocalBridge Community Build v1 — Test & Gate Report

- **Community Version:** `0.1.5-community.1`
- **Upstream Version:** `0.1.5`
- **Upstream Commit:** `c8118e73e96b05ef9090b260837a44788aeafaff`
- **OpenAI tunnel-client Version:** `0.0.11`
- **OpenAI tunnel-client Commit:** `8d55683eeef80bc5e360d95abf4692454fafc615`
- **Target Platform:** `x86_64-pc-windows-msvc` / Windows 11 x64

---

## 1. Gate Classification Summary

| Gate Category | Total Checks | PASS | FAIL | NOT RUN (Deferred) | Status |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **BUILD_GATE** | 8 | 6 | 0 | 2 (Rust/Go Host Env) | PASS (Pre-release) |
| **SECURITY_GATE** | 6 | 6 | 0 | 0 | **PASS** |
| **ENVIRONMENT_GATE** | 4 | 0 | 0 | 4 (OpenAI E2E, UAC) | **NOT RUN** (Pre-release) |

---

## 2. BUILD_GATE Status

| Check Item | Target / Command | Status | Notes |
| :--- | :--- | :--- | :--- |
| Git Workspace Cleanliness | `git status --porcelain` | **PASS** | Controlled workspace state |
| Toolchain Definition | `provenance/toolchain.json` | **PASS** | Toolchain requirements documented |
| Runtime Supply Chain Integrity | `scripts/verify-runtime.ps1` | **PASS** | All bundled runtimes match `runtime-lock.json` |
| Checksum Manifest Generation | `scripts/generate-checksums.ps1` | **PASS** | `SHA256SUMS.txt` verified |
| Frontend Dependencies | `npm install / npm ci` | **PASS** | Packages installed |
| Frontend Tests | `npm test` (vitest) | **PASS** | Model, UI, and Presentation tests |
| Frontend Typecheck & Build | `npm run build` (tsc -b && vite build) | **PASS** | Distribution bundle built to `dist/` |
| Native Tunnel Client Go Build | `scripts/build-tunnel-client.ps1` | **NOT RUN** | Go compiler deferred to CI / Go host environment |
| Rust Core & Broker Build | `cargo build --release --locked` | **NOT RUN** | Rust MSVC toolchain deferred to CI / MSVC environment |

---

## 3. SECURITY_GATE Status

| Check Item | Requirement | Status | Notes |
| :--- | :--- | :--- | :--- |
| Forbid Upstream Author Binary | No `*.upstream-localbridge.exe` or `cloudflared*` | **PASS** | Verified by `verify-runtime.ps1` |
| Fail-Closed Integrity Check | `bundle.rs` SHA256 fail-closed verification | **PASS** | Retained with no bypass |
| Sensitive Information Scan | No API keys, Bearer tokens, or private secrets in tree | **PASS** | Zero credentials leaked |
| Non-Network Listener Verification | No public `0.0.0.0` listeners in Broker/Guard | **PASS** | Localhost only (`127.0.0.1`) |
| Least Privilege Elevation | Normal execution does not bypass UAC | **PASS** | Explicit user action required |
| Staging Tree Executable Audit | No unregistered `.exe` in `runtime/` staging | **PASS** | Whitelist scan clean |

---

## 4. ENVIRONMENT_GATE Status

| Check Item | Requirement | Status | Reason Not Run |
| :--- | :--- | :--- | :--- |
| OpenAI Tunnel Client E2E | Connect to `api.openai.com` via tunnel-client | **NOT RUN** | Requires live OpenAI tunnel credentials & API key |
| MCP Tools Discovery (`tools/list`) | Query MCP server through tunnel | **NOT RUN** | Requires live tunnel connection |
| Privileged Broker UAC Dialog | Interactive Windows UAC prompt invocation | **NOT RUN** | Non-interactive execution context |
| Clean VM Installation Audit | Install on bare Windows 11 sandbox | **NOT RUN** | Requires detached VM provisioning |

---

## 5. Release Profile Evaluation

According to Section 20 of the implementation specification:
- **Status:** **Pre-release Ready**
- All `SECURITY_GATE` checks have passed.
- No `BUILD_GATE` has failed (deferred compiler gates will execute in GitHub Actions CI with full toolchains).
- `ENVIRONMENT_GATE` items are documented with explicit deferral reasons.
