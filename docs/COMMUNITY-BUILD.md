# LocalBridge Community Build Guide

## Overview

LocalBridge Community Build is an independent, auditable build of LocalBridge designed to eliminate unverified precompiled binaries while preserving 100% upstream functional and wire-protocol compatibility.

### Key Objectives
1. **Source-Built OpenAI Tunnel Client:** Built directly from official OpenAI Go source (`github.com/openai/tunnel-client`).
2. **Source-Built Privileged Broker:** Built directly from Rust source with explicit UAC elevation semantics.
3. **Supply Chain Provenance:** Every runtime executable has a pinned SHA256, origin, and lock entry in `provenance/runtime-lock.json`.
4. **Zero Ambiguous Binaries:** Complete rejection of unverified or unapproved third-party binary artifacts.

---

## Prerequisites

- **Operating System:** Windows 11 x64 (or Windows 10 21H2+)
- **Node.js:** v20.x, v22.x, or v24.x (with npm)
- **Rust Toolchain:** Rust 1.85+ with MSVC target (`x86_64-pc-windows-msvc`)
- **Go Toolchain:** Go 1.24+ (target 1.26.2)
- **Windows SDK:** Windows 10/11 SDK (for C++ build tools)

---

## Quick Start: One-Click Community Build

To run the complete automated build pipeline locally, open PowerShell as an administrator or developer console and run:

```powershell
.\scripts\build-community.ps1
```

### Script Flags:
- `-SkipTunnelBuild`: Skips compiling `tunnel-client.exe` if already built.
- `-SkipRustBuild`: Skips Rust desktop app and broker compilation.
- `-SkipFrontendBuild`: Skips React/Vite frontend compilation.
- `-GoPath <dir>`: Adds a custom Go toolchain binary folder to PATH.
- `-CargoPath <dir>`: Adds a custom Cargo/Rust binary folder to PATH.

---

## Step-by-Step Manual Build

### 1. Build OpenAI Tunnel Client from Source
```powershell
.\scripts\build-tunnel-client.ps1 -UpdateBundleRs
```
This downloads OpenAI `tunnel-client` at commit `8d55683eeef80bc5e360d95abf4692454fafc615`, builds it with `-mod=readonly -trimpath -buildvcs=false`, copies it to `runtime/tunnel-client/tunnel-client.exe`, and updates `bundle.rs` SHA256.

### 2. Verify Bundled Runtime Integrity
```powershell
.\scripts\verify-runtime.ps1
```

### 3. Build & Test Frontend
```powershell
npm ci
npm test
npm run build
```

### 4. Build Privileged Broker and Desktop App
```powershell
cargo test --manifest-path src-tauri/Cargo.toml --locked
cargo build --manifest-path src-tauri/Cargo.toml --locked --release --bin localbridge-privileged-broker
cargo build --manifest-path src-tauri/Cargo.toml --locked --release --bin localbridge
```

### 5. Generate Cryptographic Checksums
```powershell
.\scripts\generate-checksums.ps1
```
