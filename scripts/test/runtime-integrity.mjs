import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { existsSync, lstatSync, mkdtempSync, readFileSync, readdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

export const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
export function requiredFile(path) {
  if (!existsSync(path) || !lstatSync(path).isFile() || lstatSync(path).isSymbolicLink() || lstatSync(path).size === 0) {
    throw new Error(`missing, empty or unsafe required file: ${path}`);
  }
  return readFileSync(path);
}
export function verifyHash(path, expected) {
  if (!/^[0-9a-f]{64}$/i.test(expected ?? "")) throw new Error(`missing hash: ${path}`);
  if (sha256(requiredFile(path)) !== expected.toLowerCase()) throw new Error(`SHA256 mismatch: ${path}`);
}
export function filesBelow(root, prefix = "") {
  return readdirSync(root, { withFileTypes: true }).flatMap((entry) => {
    const name = prefix + entry.name;
    if (entry.isSymbolicLink()) throw new Error(`unsafe link: ${join(root, entry.name)}`);
    return entry.isDirectory() ? filesBelow(join(root, entry.name), name + "/") : [name];
  });
}
export function treeHash(root) {
  const entries = filesBelow(root).filter((name) => name.split("/").at(-1) !== "runtime-metadata.json").sort();
  return sha256(entries.map((name) => `${name}\0${sha256(readFileSync(join(root, name)))}`).join("\n"));
}
export function rejectExtras(root, allowed) {
  if (!existsSync(root)) return;
  const extras = filesBelow(root).filter((name) => !allowed.includes(name));
  if (extras.length) throw new Error(`unexpected files; remove manually:\n${extras.map((name) => join(root, name)).join("\n")}`);
}
export function section(source, name) {
  const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const matches = [...source.matchAll(new RegExp(`^\\[${escaped}\\]\\r?\\n([\\s\\S]*?)(?=^\\[|$(?![\\s\\S]))`, "gm"))];
  if (matches.length !== 1) throw new Error(`expected exactly one TOML section: ${name}`);
  return matches[0][1];
}
export function value(source, key) {
  const matches = [...source.matchAll(new RegExp(`^${key} = "([^"]+)"`, "gm"))];
  if (matches.length !== 1) throw new Error(`expected exactly one field: ${key}`);
  return matches[0][1];
}
// The `7z l -slt` header block is followed by one `Path = …` per payload entry;
// the first Path names the archive itself.
export function installerEntryPaths(listingText) {
  return [...listingText.matchAll(/^Path = (.+)$/gm)].map((match) => match[1]).slice(1);
}
export function rejectDuplicateInstallerEntries(paths) {
  const seen = new Set();
  for (const path of paths) {
    const key = path.toLowerCase();
    if (seen.has(key)) {
      throw new Error(`installer payload declares ${path} more than once; a later entry would silently overwrite the attested file`);
    }
    seen.add(key);
  }
}
function verifyInstallerPayload(root, evidence) {
  const bundleDir = join(root, "src-tauri/target/release/bundle/nsis");
  if (!existsSync(bundleDir)) return;
  const installers = filesBelow(bundleDir).filter((name) => name.endsWith("-setup.exe"));
  if (installers.length === 0) return;
  if (installers.length > 1) throw new Error(`unexpected installers; remove manually:\n${installers.join("\n")}`);
  const installer = join(bundleDir, installers[0]);
  const sevenZip = join(root, "src-tauri/target/toolbox-stage/bin/7z.exe");
  const listing = spawnSync(sevenZip, ["l", "-slt", installer], { encoding: "utf8", windowsHide: true });
  if (listing.status !== 0 || !listing.stdout) throw new Error(`installer listing failed (${listing.status ?? listing.error})`);
  const paths = installerEntryPaths(listing.stdout);
  rejectDuplicateInstallerEntries(paths);
  const brokerEntry = paths.find((path) => path.toLowerCase() === "localbridge-privileged-broker.exe");
  if (!brokerEntry) throw new Error("installer payload does not carry the attested privileged broker");
  // Extraction is retained: this repository forbids automatic bulk deletion.
  const extraction = mkdtempSync(join(tmpdir(), "localbridge-installer-verify-"));
  const unpacked = spawnSync(sevenZip, ["x", "-y", `-o${extraction}`, installer, brokerEntry], { windowsHide: true });
  if (unpacked.status !== 0) throw new Error(`installer broker extraction failed (${unpacked.status ?? unpacked.error})`);
  verifyHash(join(extraction, brokerEntry), evidence.sha256);
  console.log(`installer payload verified; broker extraction retained at ${extraction}`);
}
export function verifyRuntime(root, { bundledOnly = false, lockFile = "provenance/runtime-lock.json" } = {}) {
  const manifest = readFileSync(join(root, "runtime-manifest.toml"), "utf8");
  const lock = JSON.parse(readFileSync(resolve(root, lockFile), "utf8").replace(/^\uFEFF/, ""));
  for (const component of lock.components) {
    if (component.bundled === false) continue;
    const logical = component.runtime_path ?? component.runtime_destination;
    if (!logical) throw new Error(`missing runtime path: ${component.name}`);
    if (logical.startsWith("runtime/toolbox/") && bundledOnly) continue;
    const actual = logical.replace(/^runtime\/toolbox\//, "src-tauri/target/toolbox-stage/");
    const path = join(root, actual);
    requiredFile(path);
    const expected = component.sha256 ?? component.executable_sha256;
    if (expected) verifyHash(path, expected);
    else if (component.name !== "coding-tools-mcp") throw new Error(`unhashed component: ${component.name}`);
    for (const [name, hash] of Object.entries(component.artifacts ?? {})) verifyHash(join(dirname(path), name), hash);
    if (component.license_path) {
      requiredFile(join(root, component.license_path));
      if (component.license_sha256) verifyHash(join(root, component.license_path), component.license_sha256);
    }
  }
  for (const [name, folder] of [["python", "python"], ["coding_tools_mcp", "coding-tools-mcp"]]) {
    const block = section(manifest, name);
    const rust = readFileSync(join(root, "src-tauri/src/mcp/bundle.rs"), "utf8");
    const constant = name === "python" ? "PYTHON_TREE_SHA256" : "CODING_TOOLS_TREE_SHA256";
    const compiled = rust.match(new RegExp(`${constant}: &str =\\s*"([a-f0-9]{64})";`))?.[1];
    if (compiled !== value(block, "payload_tree_sha256")) throw new Error(`Rust/manifest tree disagreement: ${folder}`);
    if (treeHash(join(root, "runtime", folder)) !== value(block, "payload_tree_sha256")) throw new Error(`runtime tree mismatch: ${folder}`);
    const metadata = JSON.parse(requiredFile(join(root, "runtime", folder, "runtime-metadata.json")));
    const expectedMetadata = { schema_version: 1, runtime: folder === "python" ? "python-embedded" : folder, version: value(block, "version"), payload_tree_sha256: value(block, "payload_tree_sha256"), runtime_pip_present: false };
    const fields = folder === "python"
      ? { source_archive_sha256: "sha256", executable: "executable", executable_sha256: "executable_sha256", python312_dll_sha256: "python312_dll_sha256", stdlib_zip_sha256: "stdlib_zip_sha256", pth_sha256: "pth_sha256" }
      : { git_commit: "git_commit", git_tree: "git_tree", full_git_archive_sha256: "sha256", runtime_subset_archive_sha256: "runtime_subset_sha256", entry_module: "entry_module", dependency_pyjwt_version: "dependency_pyjwt_version", dependency_pyjwt_wheel_sha256: "dependency_pyjwt_wheel_sha256" };
    for (const [key, manifestKey] of Object.entries(fields)) expectedMetadata[key] = value(block, manifestKey);
    if (folder === "python") Object.assign(expectedMetadata, { isolated: true, user_site_enabled: false, external_python_fallback: false });
    for (const [key, expected] of Object.entries(expectedMetadata)) if (metadata[key] !== expected) throw new Error(`metadata mismatch: ${folder}/${key}`);
  }
  const tunnel = section(manifest, "tunnel_client");
  verifyHash(join(root, value(tunnel, "executable")), value(tunnel, "executable_sha256"));
  const rust = readFileSync(join(root, "src-tauri/src/tunnel/bundle.rs"), "utf8");
  const pinned = rust.match(/TUNNEL_CLIENT_SHA256: &str =\s*"([a-f0-9]{64})";/)?.[1];
  if (pinned !== value(tunnel, "executable_sha256")) throw new Error("Tunnel Rust/manifest hash disagreement");
  rejectExtras(join(root, "runtime/tunnel-client"), ["tunnel-client.exe", "LICENSE"]);
  const known = ["python/python.exe", "python/pythonw.exe", "tunnel-client/tunnel-client.exe"];
  const extraExecutables = filesBelow(join(root, "runtime")).filter((p) => /\.exe$/i.test(p) && !known.includes(p));
  if (extraExecutables.length) throw new Error(`unregistered runtime executables: ${extraExecutables.join(", ")}`);
  if (!bundledOnly) {
    const stage = join(root, "src-tauri/target/toolbox-stage");
    rejectExtras(stage, ["bin/aria2c.exe", "bin/7z.exe", "bin/jq.exe", "bin/curl.cmd"]);
    for (const name of ["aria2c", "seven_zip", "jq"]) {
      const block = section(manifest, `toolbox.${name}`);
      verifyHash(join(root, value(block, "executable").replace("runtime/toolbox/", "src-tauri/target/toolbox-stage/")), value(block, "executable_sha256"));
    }
    const curl = '@echo off\r\n"%SystemRoot%\\System32\\curl.exe" %*\r\n';
    verifyHash(join(stage, "bin/curl.cmd"), sha256(curl));
  const broker = join(root, "src-tauri/target/release-stage/localbridge-privileged-broker.exe");
  const evidence = JSON.parse(requiredFile(join(root, "src-tauri/target/release-stage/broker-build.json")));
  if (evidence.status !== "PASS") throw new Error("broker build incomplete");
  verifyHash(broker, evidence.sha256);
  // The installer embeds the staged, evidence-attested broker (tauri.conf
  // bundle.resources copies it from target/release-stage). The app build
  // legitimately recompiles the broker into target/release with the app
  // feature set, so that build-tree copy is not comparable to the staged
  // evidence; the property that must hold is that the installer's broker
  // source of truth stays the attested staged binary.
  const config = JSON.parse(requiredFile(join(root, "src-tauri/tauri.conf.json")));
  if (config?.bundle?.resources?.["target/release-stage/localbridge-privileged-broker.exe"] !== "localbridge-privileged-broker.exe") {
    throw new Error("installer must embed the attested staged broker; check bundle.resources");
  }
  // Config mapping alone does not prove the installer content: when a packaged
  // NSIS installer already exists in this checkout, open it and verify the
  // actually carried broker against the same evidence, and that no later
  // duplicate entry overwrites it.
  verifyInstallerPayload(root, evidence);
  rejectExtras(dirname(broker), ["localbridge-privileged-broker.exe", "broker-build.json"]);
  }
  return { status: "PASS", coverage: bundledOnly ? "bundled-only" : "bundled-and-staged" };
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const root = resolve(import.meta.dirname, "../..");
  const lockIndex = process.argv.indexOf("--lock");
  console.log(JSON.stringify(verifyRuntime(root, { bundledOnly: process.argv.includes("--bundled-only"), ...(lockIndex < 0 ? {} : { lockFile: process.argv[lockIndex + 1] }) })));
}
