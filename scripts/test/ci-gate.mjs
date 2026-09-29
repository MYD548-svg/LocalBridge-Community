import { pathToFileURL } from "node:url";

import { runStage, selectStages, validateStages } from "./process.mjs";
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
      "scripts/test/structure.test.mjs",
      "scripts/test/build-regression.test.mjs",
      "scripts/test/runtime-output.test.mjs",
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
      "--manifest-path",
      "src-tauri/Cargo.toml",
      "--locked",
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

export function main(args = process.argv.slice(2)) {
  const options = parseGateArguments(args);
  const selected = selectStages(CI_STAGES, options);
  if (options.list) {
    for (const stage of selected) process.stdout.write(`${stage.id}\t${stage.label}\n`);
    return;
  }
  if (![undefined, "bundled", "community"].includes(process.env.LOCALBRIDGE_BUILD_PROFILE)) throw new Error("Unknown build profile");
  process.env.RUSTUP_TOOLCHAIN = "1.85.0";
  const report = { commit: command("git", ["rev-parse", "HEAD"]), profile: process.env.LOCALBRIDGE_BUILD_PROFILE ?? "bundled", startedAt: new Date().toISOString(), status: "RUNNING", upstreamSuite: "NOT_RUN_IN_WINDOWS_GATE; community Actions requires the separate Linux job", stages: CI_STAGES.map(({ id }) => ({ id, status: "NOT_RUN" })), environmentAcceptance: "NOT_RUN: live ChatGPT, UAC and clean Windows installation" };
  mkdirSync(evidenceRoot, { recursive: true });
  const save = () => writeFileSync(join(evidenceRoot, "TEST-REPORT.json"), JSON.stringify(report, null, 2) + "\n");
  save();
  try {
    for (const stage of selected) {
      const entry = report.stages.find(({ id }) => id === stage.id);
      entry.status = "RUNNING";
      save();
      try { runStage(stage); entry.status = "PASS"; }
      catch (error) { entry.status = "FAIL"; entry.error = error.message; throw error; }
      finally { save(); }
    }
    report.status = selected.length === CI_STAGES.length ? "PASS" : "PARTIAL";
  } catch (error) {
    report.status = "FAIL";
    throw error;
  } finally {
    report.finishedAt = new Date().toISOString();
    save();
    if (report.status === "PASS") {
      const evidence = ["TEST-REPORT.json", "BUILD-PROVENANCE.json", "toolchains.json", "COMPILE-PREFLIGHT.json"];
      appendFileSync(join(evidenceRoot, "SHA256SUMS.txt"), evidence.map((name) => `${sha256(readFileSync(join(evidenceRoot, name)))}  tests/artifacts/ci/${name}\n`).join(""));
    }
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main();
