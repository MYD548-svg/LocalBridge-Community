import { build } from "vite";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { resolve, join } from "node:path";
import { spawnSync } from "node:child_process";
import { extensionId, makeZip, sha256, verifyExtensionArchive } from "./test/browser-extension-package.mjs";
const root = resolve(import.meta.dirname, ".."), source = join(root, "extensions/chatgpt-web");
const args = process.argv.slice(2);
if (args.length > 1 || args.some(arg => arg !== "--development")) throw new Error("unknown browser build argument");
const development = args.includes("--development");
const identity = JSON.parse(readFileSync(join(source, development ? "identity.dev.json" : "identity.json"), "utf8"));
if (extensionId(identity.key) !== identity.id || readFileSync(join(source, development ? "extension-id.dev.txt" : "extension-id.txt"), "utf8").trim() !== identity.id) throw new Error("fixed extension identity drift");
const packageVersion = JSON.parse(readFileSync(join(root, "package.json"), "utf8")).version;
const evidence = join(root, development ? "tests/artifacts/browser-extension-dev" : "tests/artifacts/browser-extension"); mkdirSync(evidence, { recursive: true });
const output = mkdtempSync(join(evidence, "build-"));
for (const name of ["content", "background", "popup"]) {
  await build({
    configFile: false, root: source, publicDir: false,
    define: { __LOCALBRIDGE_DEVELOPMENT__: JSON.stringify(development) },
    build: { outDir: output, emptyOutDir: false, sourcemap: false,
      lib: { entry: join(source, name + ".ts"), name: "LocalBridge" + name, formats: ["iife"], fileName: () => name + ".js" },
      rollupOptions: { output: { inlineDynamicImports: true } } },
  });
}
const manifest = {
  manifest_version: 3, name: "LocalBridge ChatGPT 网页工具" + (development ? "（开发版）" : ""), version: packageVersion,
  description: "通过 LocalBridge 在本机执行工具，由用户确认后把结果送回当前聊天。",
  minimum_chrome_version: "120", key: identity.key,
  permissions: ["nativeMessaging", "storage", "clipboardWrite"],
  host_permissions: ["https://chatgpt.com/*"],
  background: { service_worker: "background.js" },
  action: { default_popup: "popup.html", default_title: "LocalBridge 网页工具" },
  content_scripts: [{ matches: ["https://chatgpt.com/*"], js: ["content.js"], run_at: "document_idle", all_frames: false }],
  content_security_policy: { extension_pages: "script-src 'self'; object-src 'none'" },
};
writeFileSync(join(output, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n");
for (const name of ["popup.html", "popup.css", "INSTALL.html", "INSTALL.svg"]) copyFileSync(join(source, name), join(output, name));
const files = new Map(readdirSync(output).sort().map(name => [name, readFileSync(join(output, name))]));
const metadata = { schemaVersion: 1, version: packageVersion, protocol: 1, application: ">=0.1.5, <0.2.0",
  extensionId: identity.id, files: Object.fromEntries([...files].map(([name, bytes]) => [name, sha256(bytes)])) };
const metadataBytes = Buffer.from(JSON.stringify(metadata, null, 2) + "\n");
files.set("localbridge-extension.json", metadataBytes); writeFileSync(join(output, "localbridge-extension.json"), metadataBytes);
const archive = join(evidence, "LocalBridge-ChatGPT-Web-v" + packageVersion + ".zip");
writeFileSync(archive, makeZip(files));
verifyExtensionArchive(archive, root, development);
const git = spawnSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8", windowsHide: true });
if (git.status !== 0) throw new Error("cannot identify extension source SHA");
const status = spawnSync("git", ["status", "--porcelain", "--untracked-files=no"], { cwd: root, encoding: "utf8", windowsHide: true });
if (status.status !== 0) throw new Error("cannot identify extension worktree state");
const sourceHashes = Object.fromEntries(readdirSync(source).filter(name => /\.(ts|json|html|css|svg|txt)$/.test(name)).sort().map(name => [name, sha256(readFileSync(join(source, name)))]));
writeFileSync(join(evidence, "extension-build.json"), JSON.stringify({
  status: "PASS", commit: git.stdout.trim(), version: packageVersion, protocol: 1, extensionId: identity.id,
  dirty: Boolean(status.stdout.trim()), development, sourceHashes,
  archive, directory: output, sha256: sha256(readFileSync(archive)), builtAt: new Date().toISOString(),
}, null, 2) + "\n");
writeFileSync(join(evidence, "SHA256SUMS.txt"), sha256(readFileSync(archive)) + "  " + archive.split(/[\\/]/).at(-1) + "\n");
console.log("BROWSER_EXTENSION=PASS " + archive);
