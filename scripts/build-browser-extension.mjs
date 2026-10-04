import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { resolve, join } from "node:path";
import { spawnSync } from "node:child_process";
import { pathToFileURL } from "node:url";
import { extensionId, makeZip, sha256, verifyExtensionArchive } from "./test/browser-extension-package.mjs";
import { rejectExtras, requiredFile } from "./test/runtime-integrity.mjs";
import { applicationVersion, bundleDirectory, releaseConfiguration, verifyBundledExtension } from "./release-contract.mjs";
const root = resolve(import.meta.dirname, "..");
function inputs(repository) {
  const source = join(repository, "extensions/chatgpt-web");
  const paths = [...readdirSync(source).filter(name => /\.(ts|json|html|css|svg|txt)$/.test(name)).map(name => "extensions/chatgpt-web/" + name),
    "LICENSE", "package.json", "package-lock.json", "product-release.json", "src-tauri/Cargo.toml", "src-tauri/tauri.conf.json", "scripts/build-browser-extension.mjs", "scripts/release-contract.mjs", "scripts/test/browser-extension-package.mjs", "scripts/test/runtime-integrity.mjs"];
  return Object.fromEntries(paths.sort().map(name => [name, sha256(requiredFile(join(repository, name)))]));
}
export async function ensureBrowserExtension({ repository = root, development = false, ...options } = {}) {
  try {
    const evidence = join(repository, development ? "tests/artifacts/browser-extension-dev" : "tests/artifacts/browser-extension");
    const old = JSON.parse(requiredFile(join(evidence, "extension-build.json")));
    const head = options.checkout ? options.checkout() : (() => {
      const git = spawnSync("git", ["rev-parse", "HEAD"], { cwd: repository, encoding: "utf8", windowsHide: true });
      if (git.status !== 0) throw new Error("cannot identify extension checkout");
      return git.stdout.trim();
    })();
    if (old.status !== "PASS" || old.commit !== head || old.development !== development
      || old.version !== applicationVersion(repository) || JSON.stringify(old.buildInputs) !== JSON.stringify(inputs(repository))
      || old.archive !== join(evidence, "LocalBridge-ChatGPT-Web-v" + old.version + ".zip")
      || sha256(requiredFile(old.archive)) !== old.sha256) throw new Error("extension evidence is stale");
    verifyExtensionArchive(old.archive, repository, development);
    if (!development) {
      const bundle = verifyBundledExtension(repository);
      if (bundle.sourceCommit !== old.commit || bundle.extension.asset.sha256 !== old.sha256) throw new Error("extension bundle drift");
    }
    console.log("BROWSER_EXTENSION=REUSED verified " + old.archive);
    return old;
  } catch {
    return buildBrowserExtension({ repository, development, ...options });
  }
}
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
  const stage = bundleDirectory(repository);
  const saveBundle = value => { mkdirSync(stage, { recursive: true }); writeFileSync(join(stage, "bundle.json"), JSON.stringify(value, null, 2) + "\n"); };
  if (!development) saveBundle({ schemaVersion: 1, status: "BUILDING" });
  try {
    const config = releaseConfiguration(repository);
    const commit = checkout();
    if (!/^[a-f0-9]{40}$/.test(commit ?? "")) throw new Error("invalid extension source SHA");
    const identity = JSON.parse(readFileSync(join(source, development ? "identity.dev.json" : "identity.json"), "utf8"));
    if (extensionId(identity.key) !== identity.id || readFileSync(join(source, development ? "extension-id.dev.txt" : "extension-id.txt"), "utf8").trim() !== identity.id) throw new Error("fixed extension identity drift");
    const packageVersion = applicationVersion(repository);
    await typecheck();
    const output = mkdtempSync(join(evidence, "build-"));
    for (const name of ["content", "background", "popup"]) {
      await bundle({
        configFile: false, root: source, publicDir: false,
        define: { __LOCALBRIDGE_DEVELOPMENT__: JSON.stringify(development), __LOCALBRIDGE_VERSION__: JSON.stringify(packageVersion),
          __LOCALBRIDGE_RELEASES_URL__: JSON.stringify("https://github.com/" + config.repository + "/releases") },
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
    copyFileSync(join(repository, "LICENSE"), join(output, "LICENSE"));
    const files = new Map(readdirSync(output).sort().map(name => [name, requiredFile(join(output, name))]));
    const metadata = { schemaVersion: 1, version: packageVersion, protocol: 1, application: config.applicationCompatibility,
      repository: config.repository, sourceCommit: commit,
      extensionId: identity.id, files: Object.fromEntries([...files].map(([name, bytes]) => [name, sha256(bytes)])) };
    const metadataBytes = Buffer.from(JSON.stringify(metadata, null, 2) + "\n");
    files.set("localbridge-extension.json", metadataBytes); writeFileSync(join(output, "localbridge-extension.json"), metadataBytes);
    const archive = join(evidence, "LocalBridge-ChatGPT-Web-v" + packageVersion + ".zip");
    writeFileSync(archive, makeZip(files));
    verifyArchive(archive, repository, development);
    const sourceHashes = Object.fromEntries(readdirSync(source).filter(name => /\.(ts|json|html|css|svg|txt)$/.test(name)).sort().map(name => [name, sha256(readFileSync(join(source, name)))]));
    const record = {
      status: "PASS", commit, version: packageVersion, protocol: 1, extensionId: identity.id,
      dirty: worktree(), development, sourceHashes, buildInputs: inputs(repository),
      archive, directory: output, sha256: sha256(readFileSync(archive)), builtAt: new Date().toISOString(),
    };
    if (!development) {
      rejectExtras(stage, ["extension.zip", "bundle.json"]);
      copyFileSync(archive, join(stage, "extension.zip"));
      saveBundle({ schemaVersion: 1, status: "PASS", repository: config.repository, channel: config.channel,
        applicationVersion: packageVersion, sourceCommit: commit,
        extension: { version: packageVersion, protocol: 1, extensionId: identity.id, application: config.applicationCompatibility,
          asset: { name: archive.split(/[\\/]/).at(-1), sha256: record.sha256, size: readFileSync(archive).length } } });
      verifyBundledExtension(repository);
    }
    writeFileSync(join(evidence, "SHA256SUMS.txt"), record.sha256 + "  " + archive.split(/[\\/]/).at(-1) + "\n");
    save(record);
    console.log("BROWSER_EXTENSION=PASS " + archive);
    return record;
  } catch (error) {
    if (!development) saveBundle({ schemaVersion: 1, status: "FAIL", error: error.message });
    save({ status: "FAIL", development, error: error.message });
    throw error;
  }
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const args = process.argv.slice(2);
  if (new Set(args).size !== args.length || args.some(arg => !["--development", "--reuse-verified"].includes(arg))) throw new Error("unknown browser build argument");
  await (args.includes("--reuse-verified") ? ensureBrowserExtension : buildBrowserExtension)({ development: args.includes("--development") });
}
