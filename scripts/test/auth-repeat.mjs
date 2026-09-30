import { cargoCommand } from "./ci-gate.mjs";
import { runStage } from "./process.mjs";
for (let attempt = 1; attempt <= 10; attempt++) {
  runStage({ id: `auth-${attempt}`, label: "strict authenticated MCP probe", ...cargoCommand(["test", "--manifest-path", "src-tauri/Cargo.toml", "--locked", "--lib", "tunnel::runtime::tests::actual_tunnel_discovery_sends_the_authenticated_pep_header", "--", "--exact", "--test-threads=1", "--nocapture"]) });
}

// Preserve the original ten authenticated Tunnel probes, then cover the retained
// output regression through the permission service and the new local route.
for (let attempt = 1; attempt <= 10; attempt++) {
  runStage({ id: `stderr-${attempt}`, label: "retained stderr remains readable", ...cargoCommand(["test", "--manifest-path", "src-tauri/Cargo.toml", "--locked", "--lib", "mcp::server::tests::busy_terminal_replay_retains_readable_stderr_and_its_owner", "--", "--exact", "--test-threads=1", "--nocapture"]) });
}
runStage({ id: "local-protocol", label: "actual local pipe protocol, cancellation, permissions and ten output repetitions", ...cargoCommand(["test", "--manifest-path", "src-tauri/Cargo.toml", "--locked", "--lib", "local_connection::tests::", "--", "--test-threads=1", "--nocapture"]) });
