import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { existsSync, lstatSync, mkdtempSync, readFileSync, readdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { verifyBundledExtension } from "../release-contract.mjs";
import { verifyExtensionArchive } from "./browser-extension-package.mjs";

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
function resolveInstallerSevenZip(root) {
  // The toolbox 7z.exe is 7za (7z2602-extra), which cannot parse NSIS
  // archives (exit code 2). Full 7-Zip is preinstalled on the CI runner
  // images; the toolbox copy is only the last resort.
  const candidates = [
    join(process.env["ProgramFiles"] ?? "C:\\Program Files", "7-Zip", "7z.exe"),
    join(process.env["ProgramFiles(x86)"] ?? "C:\\Program Files (x86)", "7-Zip", "7z.exe"),
    join(root, "src-tauri/target/toolbox-stage/bin/7z.exe"),
  ];
  for (const candidate of candidates) {
    if (existsSync(candidate)) return candidate;
  }
  throw new Error("no 7-Zip installation available for installer payload verification");
}
function verifyInstallerPayload(root, evidence, adapterEvidence, hostEvidence, extensionBundle) {
  const bundleDir = join(root, "src-tauri/target/release/bundle/nsis");
  if (!existsSync(bundleDir)) return;
  const installers = filesBelow(bundleDir).filter((name) => name.endsWith("-setup.exe"));
  if (installers.length === 0) return;
  if (installers.length > 1) throw new Error(`unexpected installers; remove manually:\n${installers.join("\n")}`);
  const installer = join(bundleDir, installers[0]);
  const sevenZip = resolveInstallerSevenZip(root);
  const listing = spawnSync(sevenZip, ["l", "-slt", installer], { encoding: "utf8", windowsHide: true });
  if (listing.status !== 0 || !listing.stdout) throw new Error(`installer listing failed (${listing.status ?? listing.error})`);
  const paths = installerEntryPaths(listing.stdout);
  rejectDuplicateInstallerEntries(paths);
  const adapterEntry = paths.find((path) => path.toLowerCase() === "localbridge-mcp.exe");
  if (!adapterEntry) throw new Error("installer payload does not carry the attested local MCP adapter");
  const licenseEntry = paths.find((path) => path.replaceAll("\\", "/").toLowerCase() === "licenses/mcp-proxy-mit.txt");
  if (!licenseEntry) throw new Error("installer adapter attribution missing");
  const brokerEntry = paths.find((path) => path.toLowerCase() === "localbridge-privileged-broker.exe");
  if (!brokerEntry) throw new Error("installer payload does not carry the attested privileged broker");
  const hostEntry = paths.find((path) => path.toLowerCase() === "localbridge-browser-host.exe");
  const templateEntry = paths.find((path) => path.toLowerCase() === "native-host-template.json");
  if (!hostEntry || !templateEntry) throw new Error("installer browser gateway or manifest missing");
  const extensionEntry = paths.find(path => path.replaceAll("\\", "/").toLowerCase() === "browser-extension/extension.zip");
  const bundleEntry = paths.find(path => path.replaceAll("\\", "/").toLowerCase() === "browser-extension/bundle.json");
  if (!extensionEntry || !bundleEntry) throw new Error("installer bundled extension missing");
  // Extraction is retained: this repository forbids automatic bulk deletion.
  const extraction = mkdtempSync(join(tmpdir(), "localbridge-installer-verify-"));
  const unpacked = spawnSync(sevenZip, ["x", "-y", `-o${extraction}`, installer, brokerEntry, adapterEntry, licenseEntry], { windowsHide: true });
  if (unpacked.status !== 0) throw new Error(`installer broker extraction failed (${unpacked.status ?? unpacked.error})`);
  verifyHash(join(extraction, brokerEntry), evidence.sha256);
  verifyHash(join(extraction, adapterEntry), adapterEvidence.sha256);
  for (const entry of [hostEntry, templateEntry, extensionEntry, bundleEntry]) {
    const extracted = spawnSync(sevenZip, ["x", "-y", "-o" + extraction, installer, entry], { windowsHide: true });
    if (extracted.status !== 0) throw new Error("browser payload extraction failed");
  }
  verifyHash(join(extraction, hostEntry), hostEvidence.sha256);
  verifyHash(join(extraction, templateEntry), hostEvidence.templateSha256);
  verifyHash(join(extraction, extensionEntry), extensionBundle.extension.asset.sha256);
  verifyHash(join(extraction, bundleEntry), sha256(requiredFile(join(root, "src-tauri/target/browser-extension-stage/bundle.json"))));
  verifyHash(join(extraction, licenseEntry), sha256(requiredFile(join(root, "docs/licenses/mcp-proxy-MIT.txt"))));
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
  const adapter = join(root, "src-tauri/target/local-mcp-stage/localbridge-mcp.exe");
  const adapterEvidence = JSON.parse(requiredFile(join(root, "src-tauri/target/local-mcp-stage/adapter-build.json")));
  if (adapterEvidence.status !== "PASS") throw new Error("adapter build incomplete");
  verifyHash(adapter, adapterEvidence.sha256);
  requiredFile(join(root, "docs/licenses/mcp-proxy-MIT.txt"));
  if (config?.bundle?.resources?.["target/local-mcp-stage/localbridge-mcp.exe"] !== "localbridge-mcp.exe" || config?.bundle?.resources?.["../docs/licenses/mcp-proxy-MIT.txt"] !== "licenses/mcp-proxy-MIT.txt") throw new Error("installer must embed the attested staged adapter and license");
  rejectExtras(dirname(adapter), ["localbridge-mcp.exe", "adapter-build.json"]);
  const host = join(root, "src-tauri/target/browser-host-stage/localbridge-browser-host.exe");
  const hostEvidence = JSON.parse(requiredFile(join(dirname(host), "browser-host-build.json")));
  const identity = JSON.parse(requiredFile(join(root, "extensions/chatgpt-web/identity.json")));
  if (hostEvidence.status !== "PASS" || !/^[a-f0-9]{40}$/.test(hostEvidence.sourceSha ?? "") || hostEvidence.protocolVersion !== identity.protocol || hostEvidence.extensionId !== identity.id) throw new Error("browser host build incomplete or incompatible");
  verifyHash(host, hostEvidence.sha256);
  verifyHash(join(dirname(host), "native-host-template.json"), hostEvidence.templateSha256);
  const template = JSON.parse(requiredFile(join(dirname(host), "native-host-template.json")));
  if (template.name !== identity.host || template.path !== "localbridge-browser-host.exe" || template.type !== "stdio" || JSON.stringify(template.allowed_origins) !== JSON.stringify(["chrome-extension://" + identity.id + "/"])) throw new Error("browser host registration template drift");
  if (config?.bundle?.resources?.["target/browser-host-stage/localbridge-browser-host.exe"] !== "localbridge-browser-host.exe" || config?.bundle?.resources?.["target/browser-host-stage/native-host-template.json"] !== "native-host-template.json") throw new Error("installer must embed the attested browser gateway");
  rejectExtras(dirname(host), ["localbridge-browser-host.exe", "browser-host-build.json", "native-host-template.json"]);
  const extensionBundle = verifyBundledExtension(root);
  const extensionDirectory = join(root, "src-tauri/target/browser-extension-stage");
  const extension = verifyExtensionArchive(join(extensionDirectory, "extension.zip"), root);
  if (extensionBundle.sourceCommit !== hostEvidence.sourceSha || extension.version !== extensionBundle.extension.version
    || extension.repository !== extensionBundle.repository || extension.sourceCommit !== extensionBundle.sourceCommit) throw new Error("bundled extension source or version mismatch");
  if (config?.bundle?.resources?.["target/browser-extension-stage/extension.zip"] !== "browser-extension/extension.zip"
    || config?.bundle?.resources?.["target/browser-extension-stage/bundle.json"] !== "browser-extension/bundle.json") throw new Error("installer must embed the verified extension ZIP and bundle descriptor");
  rejectExtras(extensionDirectory, ["extension.zip", "bundle.json"]);
  verifyInstallerPayload(root, evidence, adapterEvidence, hostEvidence, extensionBundle);
  rejectExtras(dirname(broker), ["localbridge-privileged-broker.exe", "broker-build.json"]);
  }
  return { status: "PASS", coverage: bundledOnly ? "bundled-only" : "bundled-and-staged" };
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const root = resolve(import.meta.dirname, "../..");
  const lockIndex = process.argv.indexOf("--lock");
  console.log(JSON.stringify(verifyRuntime(root, { bundledOnly: process.argv.includes("--bundled-only"), ...(lockIndex < 0 ? {} : { lockFile: process.argv[lockIndex + 1] }) })));
}
