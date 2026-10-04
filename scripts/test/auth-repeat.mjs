import { pathToFileURL } from "node:url";
import { cargoCommand } from "./ci-gate.mjs";
import { runReportedStages } from "./stage-runner.mjs";

export function authStages() {
  const stage = (id, label, filter, exact = false) => ({ id, label, requireTests: true, timeoutMs: 10 * 60_000, ...cargoCommand(["test", "--manifest-path", "src-tauri/Cargo.toml", "--locked", "--lib", filter, "--", ...(exact ? ["--exact"] : []), "--test-threads=1", "--nocapture"]) });
  return [
    ...[
      ["output-read", "output_read_"],
      ["policy-shutdown", "mcp::server::tests::policy_shutdown_"],
      ["local-registration", "local_connection::registration::tests::"],
      ["local-pipe-connect", "local_connection::pipe::tests::"],
    ].map(([id, filter]) => stage(id, "local lifecycle and connection behavior", filter)),
    ...Array.from({ length: 10 }, (_, index) => stage(`auth-${index + 1}`, "strict authenticated MCP probe", "tunnel::runtime::tests::actual_tunnel_discovery_sends_the_authenticated_pep_header", true)),
    ...Array.from({ length: 10 }, (_, index) => stage(`stderr-${index + 1}`, "retained stderr remains readable", "mcp::server::tests::busy_terminal_replay_retains_readable_stderr_and_its_owner", true)),
    stage("local-protocol", "actual local pipe protocol, cancellation, permissions and ten output repetitions", "local_connection::tests::"),
  ];
}

export async function main() {
  const stages = authStages();
  const report = { status: "RUNNING", startedAt: new Date().toISOString(), stages: stages.map(({ id }) => ({ id, status: "NOT_RUN" })) };
  return runReportedStages(stages, report, { reportPath: "tests/artifacts/ci/AUTH-REPEAT.json", logDirectory: "tests/artifacts/ci/logs/auth" });
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) await main();
