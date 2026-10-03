import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { resolve, join } from "node:path";
import { spawnSync } from "node:child_process";
import { pathToFileURL } from "node:url";
import { extensionId, makeZip, sha256, verifyExtensionArchive } from "./test/browser-extension-package.mjs";
import { requiredFile } from "./test/runtime-integrity.mjs";
const root = resolve(import.meta.dirname, "..");
export async function buildBrowserExtension({
  repository = root, development = false,
  typecheck = () => {
    const result = spawnSync(process.execPath, [join(repository, "node_modules/typescript/bin/tsc"), "-p", "extensions/chatgpt-web/tsconfig.json"], { cwd: repository, stdio: "inherit", windowsHide: true });
    if (result.status !== 0) throw new Error(`extension typecheck failed (${result.status ?? result.error})`);
  },
  bundle = async (options) => (await import("vite")).build(options),
  verifyArchive = verifyExtensionArchive,
  checkout = () => {
    const git = spawnSync("git", ["rev-parse", "HEAD"], { cwd: repository, encoding: "utf8", windowsHide: true });
    if (git.status !== 0) throw new Error("cannot identify extension source SHA");
    return git.stdout.trim();
  },
  worktree = () => {
    const result = spawnSync("git", ["status", "--porcelain", "--untracked-files=no"], { cwd: repository, encoding: "utf8", windowsHide: true });
    if (result.status !== 0) throw new Error("cannot identify extension worktree state");
    return Boolean(result.stdout.trim());
  },
} = {}) {
  const source = join(repository, "extensions/chatgpt-web");
  const evidence = join(repository, development ? "tests/artifacts/browser-extension-dev" : "tests/artifacts/browser-extension");
  mkdirSync(evidence, { recursive: true });
  const save = (record) => writeFileSync(join(evidence, "extension-build.json"), JSON.stringify(record, null, 2) + "\n");
  // Includes identity validation, typecheck, bundler loading and checksum writing.
  save({ status: "BUILDING", development });
  try {
    const commit = checkout();
    if (!/^[a-f0-9]{40}$/.test(commit ?? "")) throw new Error("invalid extension source SHA");
    const identity = JSON.parse(readFileSync(join(source, development ? "identity.dev.json" : "identity.json"), "utf8"));
    if (extensionId(identity.key) !== identity.id || readFileSync(join(source, development ? "extension-id.dev.txt" : "extension-id.txt"), "utf8").trim() !== identity.id) throw new Error("fixed extension identity drift");
    const packageVersion = JSON.parse(readFileSync(join(repository, "package.json"), "utf8")).version;
    await typecheck();
    const output = mkdtempSync(join(evidence, "build-"));
    for (const name of ["content", "background", "popup"]) {
      await bundle({
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
    const files = new Map(readdirSync(output).sort().map(name => [name, requiredFile(join(output, name))]));
    const metadata = { schemaVersion: 1, version: packageVersion, protocol: 1, application: ">=0.1.5, <0.2.0",
      extensionId: identity.id, files: Object.fromEntries([...files].map(([name, bytes]) => [name, sha256(bytes)])) };
    const metadataBytes = Buffer.from(JSON.stringify(metadata, null, 2) + "\n");
    files.set("localbridge-extension.json", metadataBytes); writeFileSync(join(output, "localbridge-extension.json"), metadataBytes);
    const archive = join(evidence, "LocalBridge-ChatGPT-Web-v" + packageVersion + ".zip");
    writeFileSync(archive, makeZip(files));
    verifyArchive(archive, repository, development);
    const sourceHashes = Object.fromEntries(readdirSync(source).filter(name => /\.(ts|json|html|css|svg|txt)$/.test(name)).sort().map(name => [name, sha256(readFileSync(join(source, name)))]));
    const record = {
      status: "PASS", commit, version: packageVersion, protocol: 1, extensionId: identity.id,
      dirty: worktree(), development, sourceHashes,
      archive, directory: output, sha256: sha256(readFileSync(archive)), builtAt: new Date().toISOString(),
    };
    writeFileSync(join(evidence, "SHA256SUMS.txt"), record.sha256 + "  " + archive.split(/[\\/]/).at(-1) + "\n");
    save(record);
    console.log("BROWSER_EXTENSION=PASS " + archive);
    return record;
  } catch (error) {
    save({ status: "FAIL", development, error: error.message });
    throw error;
  }
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const args = process.argv.slice(2);
  if (args.length > 1 || args.some(arg => arg !== "--development")) throw new Error("unknown browser build argument");
  await buildBrowserExtension({ development: args.includes("--development") });
}
