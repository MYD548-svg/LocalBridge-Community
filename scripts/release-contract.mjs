import { createHash } from "node:crypto";
import { lstatSync, readFileSync } from "node:fs";
import { join } from "node:path";

export const digest = bytes => createHash("sha256").update(bytes).digest("hex");
export const numericVersion = value => typeof value === "string" && /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(value)
  && value.split(".").every(part => Number(part) <= 65535);
export function compatibleApplication(version, range) {
  const bounds = typeof range === "string" && range.match(/^>=(\d+\.\d+\.\d+), <(\d+\.\d+\.\d+)$/);
  if (!bounds || !numericVersion(version) || !numericVersion(bounds[1]) || !numericVersion(bounds[2])) return false;
  const compare = (left, right) => { const a = left.split(".").map(Number), b = right.split(".").map(Number); for (let index = 0; index < 3; index++) if (a[index] !== b[index]) return a[index] - b[index]; return 0; };
  return compare(bounds[1], bounds[2]) < 0 && compare(version, bounds[1]) >= 0 && compare(version, bounds[2]) < 0;
}
export function readRequired(path) {
  const stat = lstatSync(path);
  if (!stat.isFile() || stat.isSymbolicLink() || !stat.size) throw new Error(`missing or unsafe release file: ${path}`);
  return readFileSync(path);
}
export function releaseConfiguration(root) {
  const config = JSON.parse(readRequired(join(root, "product-release.json")));
  if (config.schemaVersion !== 1 || !/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(config.repository ?? "")
    || config.channel !== "community" || config.tagSuffix !== "-community.1" || config.manifestAsset !== "localbridge-release.json"
    || !/^>=\d+\.\d+\.\d+, <\d+\.\d+\.\d+$/.test(config.applicationCompatibility ?? "")) throw new Error("invalid product release configuration");
  return config;
}
export function applicationVersion(root) {
  const version = JSON.parse(readRequired(join(root, "package.json"))).version;
  const cargo = readRequired(join(root, "src-tauri/Cargo.toml")).toString().match(/^version = "([^"]+)"/m)?.[1];
  const tauri = JSON.parse(readRequired(join(root, "src-tauri/tauri.conf.json"))).version;
  if (!numericVersion(version) || version !== cargo || version !== tauri || !compatibleApplication(version, releaseConfiguration(root).applicationCompatibility)) throw new Error("application version must match npm, Cargo and Tauri and be a legal browser numeric version");
  return version;
}
export const releaseTag = (config, version) => "v" + version + config.tagSuffix;
export const bundleDirectory = root => join(root, "src-tauri/target/browser-extension-stage");
export function verifyBundledExtension(root) {
  const config = releaseConfiguration(root);
  const identity = JSON.parse(readRequired(join(root, "extensions/chatgpt-web/identity.json")));
  const directory = bundleDirectory(root);
  const bytes = readRequired(join(directory, "extension.zip"));
  const bundle = JSON.parse(readRequired(join(directory, "bundle.json")));
  const extension = bundle.extension;
  if (bundle.schemaVersion !== 1 || bundle.status !== "PASS" || bundle.repository !== config.repository || bundle.channel !== config.channel
    || bundle.applicationVersion !== applicationVersion(root) || !/^[a-f0-9]{40}$/.test(bundle.sourceCommit ?? "")
    || !numericVersion(extension?.version) || extension.protocol !== identity.protocol || extension.extensionId !== identity.id
    || extension.application !== config.applicationCompatibility || extension.asset?.name !== "LocalBridge-ChatGPT-Web-v" + extension.version + ".zip"
    || extension.asset.size !== bytes.length || bytes.length > 32 * 1024 * 1024 || extension.asset.sha256 !== digest(bytes)) {
    throw new Error("bundled extension missing, corrupt or incompatible");
  }
  return bundle;
}
