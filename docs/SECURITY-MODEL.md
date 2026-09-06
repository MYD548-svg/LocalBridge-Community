# LocalBridge Community Build — Security Model

## 1. Trust Boundaries & Principles

LocalBridge connects remote ChatGPT sessions to local system tools and filesystems. The Community Build reinforces the following security boundaries:

```text
 ChatGPT (Cloud)
       │
       ▼ (WSS Encrypted Tunnel)
 OpenAI Tunnel Service
       │
       ▼
 tunnel-client.exe (Community Built, Source Verified)
       │
       ▼ (127.0.0.1 HTTP loopback + Bearer Token Header)
 LocalBridge MCP Guard (Rust App)
       │
       ├─────────────────────────────────┐
       ▼ (Workspace Sandboxed Exec)      ▼ (Explicit Elevation IPC)
 Standard Tools / Python Runtime    Privileged Broker (UAC Prompt)
```

---

## 2. Hard Security Rules

1. **No External Network Listeners:**
   - All server listeners (`MCP Guard`, health checks, broker IPC) bind strictly to `127.0.0.1` or named pipes.
   - `0.0.0.0` listeners are strictly prohibited.

2. **Fail-Closed Runtime Verification:**
   - Before executing `tunnel-client.exe`, `src-tauri/src/tunnel/bundle.rs` verifies its SHA256 against `TUNNEL_CLIENT_SHA256`.
   - If any hash mismatch is detected, execution immediately aborts with `TunnelError::RuntimeChecksumMismatch`.

3. **No Secrets in Process Arguments or Disk Files:**
   - API keys, tunnel tokens, and MCP Guard bearer tokens are injected exclusively via child process environment variables.
   - Secrets are never placed in CLI argv (which could leak via process monitoring or ETW).
   - Configuration files never store plaintext bearer credentials.

4. **Privileged Broker Isolation:**
   - The privileged broker runs only when explicitly invoked by user action via Windows UAC.
   - Broker IPC is secured via Windows Named Pipe ACLs restricted to the current user's session.
   - Standard mode cannot unilaterally escalate without invoking the OS elevation prompt.
