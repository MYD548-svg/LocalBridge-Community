import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { applicationVersion, releaseConfiguration } from "../release-contract.mjs";
import { makeZip, sha256 } from "./browser-extension-package.mjs";
export const root = resolve(import.meta.dirname, "../..");
export function put(root, name, bytes) { const path = join(root, name); mkdirSync(dirname(path), { recursive: true }); writeFileSync(path, bytes); return path; }
export function copyReleaseInputs(repository) {
  for (const name of ["LICENSE", "package.json", "package-lock.json", "product-release.json", "src-tauri/Cargo.toml", "src-tauri/tauri.conf.json", "scripts/build-browser-extension.mjs", "scripts/release-contract.mjs", "scripts/test/browser-extension-package.mjs", "scripts/test/runtime-integrity.mjs", "extensions/chatgpt-web/identity.json", "extensions/chatgpt-web/identity.dev.json", "extensions/chatgpt-web/extension-id.txt", "extensions/chatgpt-web/extension-id.dev.txt", "extensions/chatgpt-web/INSTALL.html", "extensions/chatgpt-web/INSTALL.svg"]) {
    const target = join(repository, name); mkdirSync(dirname(target), { recursive: true }); copyFileSync(join(root, name), target);
  }
}
export function stageFixtureExtension(repository, sourceCommit = "a".repeat(40)) {
  const config = releaseConfiguration(repository), version = applicationVersion(repository);
  const identity = JSON.parse(readFileSync(join(repository, "extensions/chatgpt-web/identity.json")));
  const manifest = { manifest_version: 3, version, key: identity.key, permissions: ["nativeMessaging", "storage", "clipboardWrite"], host_permissions: ["https://chatgpt.com/*"],
    background: { service_worker: "background.js" }, action: { default_popup: "popup.html" }, content_scripts: [{ matches: ["https://chatgpt.com/*"], js: ["content.js"], all_frames: false }] };
  const files = new Map([["manifest.json", Buffer.from(JSON.stringify(manifest))], ...["background.js", "content.js", "popup.js", "popup.html", "popup.css"].map(name => [name, Buffer.from("fixture")])]);
  for (const name of ["INSTALL.html", "INSTALL.svg"]) files.set(name, readFileSync(join(repository, "extensions/chatgpt-web", name)));
  files.set("LICENSE", readFileSync(join(repository, "LICENSE")));
  const metadata = { schemaVersion: 1, version, protocol: identity.protocol, extensionId: identity.id, application: config.applicationCompatibility, repository: config.repository, sourceCommit,
    files: Object.fromEntries([...files].map(([name, bytes]) => [name, sha256(bytes)])) };
  files.set("localbridge-extension.json", Buffer.from(JSON.stringify(metadata)));
  const bytes = makeZip(files);
  const bundle = { schemaVersion: 1, status: "PASS", repository: config.repository, channel: config.channel, sourceCommit, applicationVersion: version,
    extension: { version, protocol: identity.protocol, extensionId: identity.id, application: config.applicationCompatibility, asset: { name: `LocalBridge-ChatGPT-Web-v${version}.zip`, sha256: sha256(bytes), size: bytes.length } } };
  put(repository, "src-tauri/target/browser-extension-stage/extension.zip", bytes);
  put(repository, "src-tauri/target/browser-extension-stage/bundle.json", JSON.stringify(bundle));
  return bundle;
}
