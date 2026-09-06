# Upstream Synchronization Policy

## 1. Upstream Remotes

The repository maintains two canonical references:
- `upstream`: `https://github.com/zephyr7030/LocalBridge.git` (Official Upstream)
- `origin`: Community Build Fork repository

---

## 2. Synchronization Workflow

When a new version or commit is published upstream, follow this safe merge process:

1. **Fetch Upstream Changes:**
   ```powershell
   git fetch upstream main --tags
   ```

2. **Create Sync Branch:**
   ```powershell
   git checkout -b upstream-sync/v0.x.y
   ```

3. **Diff and Security Audit:**
   Compare changes, paying close attention to:
   - `src-tauri/src/tunnel/bundle.rs` (checksum verification)
   - `src-tauri/src/mcp/guard.rs` (bearer authentication)
   - `src-tauri/src/privilege/` (broker IPC and elevation logic)
   - `runtime-manifest.toml` (bundled dependencies)

4. **Rebase or Merge:**
   Keep community build infrastructure in `scripts/`, `provenance/`, and `docs/` intact.

5. **Re-run Gate Pipeline:**
   ```powershell
   .\scripts\build-community.ps1
   ```

6. **Update UPSTREAM-DIFF.md and TEST-REPORT.md:**
   Document any upstream changes before tagging the next `0.x.y-community.1` release.
