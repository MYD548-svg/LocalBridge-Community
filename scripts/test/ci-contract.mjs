// Shared evidence contract. Keep this leaf free of executors and side effects:
// the artifacts CLI loads release verification while the gate is still running.
export const CI_STAGE_IDS = Object.freeze([
  "toolchains",
  "dependencies",
  "tunnel-source",
  "bundled-integrity",
  "test-base",
  "format",
  "public-release",
  "licenses",
  "schema44",
  "frontend-test",
  "frontend-build",
  "runtime-resources",
  "staged-integrity",
  "auth-repeat",
  "rust-test",
  "rust-clippy",
  "nsis-package",
  "package-integrity",
  "artifacts",
]);
