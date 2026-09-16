# Community build and CI

## Requirements

Windows x64, Git on PATH, Node.js 24 (minimum 22), Rust **1.85.0** with rustfmt/clippy and the MSVC target, Visual C++ build tools and Windows SDK. Community builds additionally require Go **1.26.2**. No administrator shell is required for building.

The only ordered gate is `scripts/test/ci-gate.mjs`. Both Windows workflows and the local wrapper call it. Never duplicate individual gate commands in a workflow.

```powershell
# Bundled Tunnel profile
node scripts/test/ci-gate.mjs

# Build pinned Tunnel source, then execute the same gate
.\scripts\build-community.ps1

# List stages or diagnose one stage (reported PARTIAL, not full PASS)
node scripts/test/ci-gate.mjs --list
node scripts/test/ci-gate.mjs --only bundled-integrity
```

The wrapper accepts `-GoPath` and `-CargoPath`; skip-build switches were removed. Every required command must succeed. Resources are prepared before Cargo tests: pinned toolbox files first, then a broker bootstrap placeholder, release broker compilation, and replacement with the nonempty compiled binary. A failed broker attempt invalidates prior success evidence.

## Source and runtime integrity

Tunnel uses the fixed commit in the manifest. Downloads use a unique Git checkout; `-WorkDir` must point to an exact, clean Git repository at that commit. No wildcard directory selection is permitted. `-UpdateBundleRs` registers only the Tunnel binary/hash fields after a successful build. Without that flag, the binary remains an unregistered build artifact. Generated metadata is UTF-8 without BOM.

`verify-runtime.ps1 -BundledOnly` verifies repository runtimes only. Without that switch it also requires the actual toolbox stage and broker build evidence. Missing, empty, changed or unexpected required payloads fail verification. Toolboxes are checked at `src-tauri/target/toolbox-stage`, not at a nonexistent repository runtime directory. Python and Coding Runtime trees are checked against both manifest and Rust pins. The existing license inventory stage verifies distribution notices and dependency licenses.

Resource scripts do not recursively delete directories. Valid pinned downloads are reused; extraction uses unique directories. Unexpected staged files stop the build and print their paths for manual cleanup. Regression fixtures are also retained for manual cleanup.

## Platform tests and authentication

The community workflow runs the complete pinned upstream Go suite on Linux before the Windows job. Windows source compilation explicitly reports the upstream suite as not run on Windows; this is not a test PASS. Windows still runs the strict real-Tunnel MCP authentication test ten times, followed by the complete serial Rust suite and Clippy.

The MCP probe handles multiple connections and completes initialization. It checks actual `/mcp` requests separately from discovery traffic, records only method/path and authentication booleans, isolates inherited proxies, and stops its subprocess on failure. The Guard regression requires HTTP 401 for missing/wrong Bearer and HTTP 200 for a correct initialized request. No production authentication bypass is introduced.

## Output and acceptance

Each invocation writes `tests/artifacts/ci/TEST-REPORT.json` with commit, profile and per-stage PASS/FAIL/NOT_RUN. A partial invocation cannot count as a full build. Successful packaging additionally generates `BUILD-PROVENANCE.json`, `toolchains.json` and `SHA256SUMS.txt` there, covering the installer, binaries, resources and evidence. An old installer cannot satisfy a new run. Root-level provenance/checksums are baseline information, not current release evidence.

Diagnostics upload even on failure. Verified installer uploads run only after success. No Release is published automatically. Full acceptance requires both workflows succeeding for the same repaired commit; local edits alone do not establish this.

Live ChatGPT connection, interactive UAC and clean Windows installation remain separate NOT_RUN acceptance items. A successful NSIS build proves packaging only.
