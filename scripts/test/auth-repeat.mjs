import { cargoCommand } from "./ci-gate.mjs";
import { runStage } from "./process.mjs";
for (let attempt = 1; attempt <= 10; attempt++) {
  runStage({ id: `auth-${attempt}`, label: "strict authenticated MCP probe", ...cargoCommand(["test", "--manifest-path", "src-tauri/Cargo.toml", "--locked", "--lib", "tunnel::runtime::tests::actual_tunnel_discovery_sends_the_authenticated_pep_header", "--", "--exact", "--test-threads=1", "--nocapture"]) });
}
