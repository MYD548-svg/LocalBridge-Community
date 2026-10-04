# Community build and CI

Product distribution is configured in `product-release.json`. Installers now include the verified release extension ZIP. Main/release branches use Community Build; other branches use CI. Tag pushes do not rebuild or publish. A separate manual publication workflow reuses a successful Community Build candidate. See [paired distribution](PRODUCT-DISTRIBUTION.md).

## Requirements

Windows x64, Git on PATH, Node.js 24 (minimum 22), Rust **1.85.0** with rustfmt/clippy and the MSVC target, Visual C++ build tools and Windows SDK. Community builds additionally require Go **1.26.2**. No administrator shell is required for building.

The only ordered gate is `scripts/test/ci-gate.mjs`. Both Windows workflows and the local wrapper call it. Never duplicate individual gate commands in a workflow.

The base stage also runs `scripts/test/ui-projection-contract.test.mjs`. This portable Rust 1.85 check compiles the actual UI projection declarations and original JSON fixture, checks optional update failures, and verifies that an unrelated fault field is rejected by the compiler. Run it directly with `node --test scripts/test/ui-projection-contract.test.mjs` from the repository root. Its locked dependencies match the application lock; Cargo may fetch missing verified dependencies on a fresh runner. Source snapshots, hashes and compiler logs are retained with diagnostics; compiler outputs stay outside the diagnostics tree. An `EXPECTED_REJECTION` case records the intentional invalid temporary copy, not a product failure. Full Windows compilation and behavioral tests remain mandatory.

The base stage runs `node --test scripts/test/artifacts-cli.test.mjs` before native resource builds. Fresh Node processes execute the unchanged artifacts CLI with copied source/configuration, a local fixture Git commit, a valid extension ZIP and synthetic binary bytes. Both profiles must generate the complete candidate and matching checksums; a separate verify CLI accepts only the completed community report with all nineteen unique PASS stages. This checks delivery contracts, not real Windows executables or NSIS contents. Commands, input hashes, exit codes and output logs remain under `tests/artifacts/ci/logs/artifacts-cli`; detailed records mark intentional failures as `expectedOutcome: REJECTION`. Fixture repositories stay outside the diagnostic upload tree and are preserved. The pure stage-ID contract in `scripts/test/ci-contract.mjs` is checked against the executor's stage order; release verification imports this leaf rather than the gate, avoiding an artifacts-entry module wait cycle.

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

Diagnostics upload even on failure. Verified installer uploads run only after success. No Release is published automatically. A formal release requires a successful Community Build for the exact source commit; CI supplies ordinary branch candidates. Local edits alone do not establish Windows packaging success.

The same nineteen stages now include the ChatGPT extension tests/build/ZIP and the standalone browser host. Resource preflight compiles and lints the browser-host feature; the serial Rust stage also executes the new native-process and authenticated named-pipe tests. NSIS embeds the attested host and fixed-origin manifest template, writes an absolute manifest, and registers Edge/Chrome in both Windows registry views with ownership checks. Post-package verification extracts the host/template and checks their hashes as well as duplicate payload entries.

Candidate artifacts include the installer, extension ZIP, Chinese HTML/SVG guide, checksums and provenance bound to the current checkout SHA. Development builds use node scripts/build-browser-extension.mjs --development and the browser-host-dev Cargo feature in an isolated target directory; their identity and host registration must remain separate. They are not bundled in release artifacts.

For codex/chatgpt-web-integration without a PR, one ordinary push is expected to trigger CI only (one Windows job); Community handles main/release branches and PRs targeting them. Tags trigger neither build workflow. This task stops after the push and remote SHA comparison. Cloud results are UNVERIFIED and formal release acceptance is deferred.

Live ChatGPT connection, interactive UAC and clean Windows installation remain separate NOT_RUN acceptance items. A successful NSIS build proves packaging only.
