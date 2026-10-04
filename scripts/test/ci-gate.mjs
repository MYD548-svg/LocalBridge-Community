import { pathToFileURL } from "node:url";

import { selectStages, validateStages } from "./process.mjs";
import { runReportedStages } from "./stage-runner.mjs";
import { appendFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { command, evidenceRoot } from "./build-evidence.mjs";
import { sha256 } from "./runtime-integrity.mjs";

export function cargoCommand(args, environment = process.env) {
  const targetDir = environment.LOCALBRIDGE_CARGO_TARGET_DIR?.trim();
  return {
    program: "cargo",
    args: [
      "+1.85.0",
      args[0],
      ...(targetDir ? ["--target-dir", targetDir] : []),
      ...args.slice(1),
    ],
  };
}

const cargo = (...args) => cargoCommand(args);
const node = (...args) => ({ program: process.execPath, args });

export const CI_STAGES = validateStages([
  { id: "toolchains", label: "required build toolchains", ...node("scripts/test/build-evidence.mjs", "toolchains") },
  { id: "dependencies", label: "locked frontend dependencies", program: "npm", args: ["ci"] },
  { id: "tunnel-source", label: "profile-specific Tunnel source", ...node("scripts/test/build-evidence.mjs", "tunnel-source") },
  { id: "bundled-integrity", label: "bundled runtime integrity", ...node("scripts/test/runtime-integrity.mjs", "--bundled-only") },
  {
    id: "test-base",
    label: "test infrastructure contract",
    ...node(
      "--test",
      "tests/black-box/chatgpt/client.test.mjs",
      "tests/black-box/chatgpt/command_lifecycle.test.mjs",
      "tests/black-box/chatgpt/output_reference.test.mjs",
      "scripts/test/ci-gate.test.mjs",
      "scripts/test/stage-runner.test.mjs",
      "scripts/test/structure.test.mjs",
      "scripts/test/build-regression.test.mjs",
      "scripts/test/runtime-output.test.mjs",
      "scripts/test/adapter-client.test.mjs",
      "scripts/test/browser-extension-package.test.mjs",
    ),
  },
  {
    id: "format",
    label: "public source formatting",
    ...node("scripts/public-release/preflight.mjs", "format-check"),
  },
  {
    id: "public-release",
    label: "public export policy",
    ...node("tests/integration/release-preflight/public_release.test.mjs"),
  },
  {
    id: "licenses",
    label: "dependency license policy",
    ...node("scripts/public-release/preflight.mjs", "verify-license"),
  },
  {
    id: "schema44",
    label: "schema44 architecture residue scan",
    ...node("scripts/verify-schema44/index.mjs"),
  },
  {
    id: "frontend-test",
    label: "frontend unit tests",
    program: "npm",
    args: ["test"],
  },
  {
    id: "frontend-build",
    label: "frontend typecheck and build",
    program: "npm",
    args: ["run", "build"],
  },
  {
    id: "runtime-resources",
    label: "pinned runtime resources",
    ...node("scripts/prepare-lb018-resources.mjs", "--compile-preflight"),
  },
  {
    id: "staged-integrity",
    label: "actual packaged runtime integrity",
    ...node("scripts/test/runtime-integrity.mjs"),
  },
  {
    id: "auth-repeat",
    label: "authenticated Tunnel probe repeated ten times",
    ...node("scripts/test/auth-repeat.mjs"),
  },
  {
    id: "rust-test",
    label: "schema44 behavioral invariants and Rust tests",
    ...cargo(
      "test",
      "--quiet",
      "--no-fail-fast",
      "--manifest-path",
      "src-tauri/Cargo.toml",
      "--locked",
      "--features",
      "browser-host",
      "--",
      "--test-threads=1",
    ),
  },
  {
    id: "rust-clippy",
    label: "Rust lint gate",
    ...cargo(
      "clippy",
      "--manifest-path",
      "src-tauri/Cargo.toml",
      "--locked",
      "--all-targets",
      "--features",
      "browser-host",
      "--",
      "-D",
      "warnings",
    ),
  },
  {
    id: "nsis-package",
    label: "production NSIS package",
    ...node("node_modules/@tauri-apps/cli/tauri.js", "build", "--bundles", "nsis"),
  },
  { id: "package-integrity", label: "post-build runtime integrity", ...node("scripts/test/runtime-integrity.mjs") },
  { id: "artifacts", label: "installer checksums and provenance", ...node("scripts/test/build-evidence.mjs", "artifacts") },
]);

// Collect independent failures in the same job. Native tests require verified
// staging; packaging requires every preceding mandatory gate to have passed.
const prerequisites = {
  "tunnel-source": ["toolchains"],
  "bundled-integrity": ["tunnel-source"],
  "test-base": ["dependencies"],
  format: ["toolchains"],
  licenses: ["toolchains", "dependencies"],
  "frontend-test": ["dependencies"],
  "frontend-build": ["dependencies"],
  "runtime-resources": ["toolchains", "bundled-integrity", "test-base"],
  "staged-integrity": ["runtime-resources"],
  "auth-repeat": ["staged-integrity"],
  "rust-test": ["staged-integrity"],
  "rust-clippy": ["runtime-resources"],
  "nsis-package": CI_STAGES.slice(0, CI_STAGES.findIndex(({ id }) => id === "nsis-package")).map(({ id }) => id),
  "package-integrity": ["nsis-package"],
  artifacts: ["package-integrity"],
};
for (const stage of CI_STAGES) {
  stage.needs = prerequisites[stage.id] ?? [];
  stage.timeoutMs = stage.id === "runtime-resources" ? 90 * 60_000 : stage.id === "auth-repeat" ? 60 * 60_000 : 30 * 60_000;
}

export function parseGateArguments(args) {
  const options = {};
  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (argument === "--list") options.list = true;
    else if (argument === "--only") options.only = args[++index];
    else if (argument === "--from") options.from = args[++index];
    else if (argument === "--through") options.through = args[++index];
    else throw new Error(`unknown test gate argument: ${argument}`);
  }
  if (options.only && (options.from || options.through)) {
    throw new Error("--only is mutually exclusive with --from/--through");
  }
  return options;
}

export async function main(args = process.argv.slice(2)) {
  const options = parseGateArguments(args);
  const selected = selectStages(CI_STAGES, options);
  if (options.list) {
    for (const stage of selected) process.stdout.write(`${stage.id}\t${stage.label}\n`);
    return;
  }
  if (![undefined, "bundled", "community"].includes(process.env.LOCALBRIDGE_BUILD_PROFILE)) throw new Error("Unknown build profile");
  process.env.RUSTUP_TOOLCHAIN = "1.85.0";
  const report = { commit: command("git", ["rev-parse", "HEAD"]), profile: process.env.LOCALBRIDGE_BUILD_PROFILE ?? "bundled", startedAt: new Date().toISOString(), status: "RUNNING", upstreamSuite: "NOT_RUN_IN_WINDOWS_GATE; community Actions requires the separate Linux job", releaseNoConsole: process.env.LOCALBRIDGE_RELEASE_EXE ? "EXTERNAL_INPUT: not attested as this gate's newly built EXE" : "NOT_RUN: pre-package Rust tests do not receive a newly built release EXE", stages: CI_STAGES.map(({ id, program, args, needs, timeoutMs }) => ({ id, program, args, needs, timeoutMs, status: "NOT_RUN", exitCode: null })), environmentAcceptance: "NOT_RUN: live ChatGPT, UAC and clean Windows installation" };
  mkdirSync(evidenceRoot, { recursive: true });
  const save = () => writeFileSync(join(evidenceRoot, "TEST-REPORT.json"), JSON.stringify(report, null, 2) + "\n");
  save();
  try {
    await runReportedStages(selected, report, { reportPath: "tests/artifacts/ci/TEST-REPORT.json" });
  } finally {
    report.finishedAt = new Date().toISOString();
    save();
    if (report.status === "PASS") {
      const evidence = ["TEST-REPORT.json", "BUILD-PROVENANCE.json", "toolchains.json", "COMPILE-PREFLIGHT.json"];
      appendFileSync(join(evidenceRoot, "SHA256SUMS.txt"), evidence.map((name) => `${sha256(readFileSync(join(evidenceRoot, name)))}  tests/artifacts/ci/${name}\n`).join(""));
    }
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) await main();
