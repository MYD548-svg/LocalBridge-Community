import assert from "node:assert/strict";
import test from "node:test";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { extensionId, makeZip, readStoredZip, safePackageName, sha256, verifyExtensionFiles, verifyExtensionArchive } from "./browser-extension-package.mjs";
import { copyReleaseInputs } from "./product-release-fixture.mjs";
import { verifyBundledExtension } from "../release-contract.mjs";
import { buildBrowserExtension, ensureBrowserExtension } from "../build-browser-extension.mjs";
test("stored release ZIP roundtrips Unicode and rejects corruption and aliases", () => {
  const files = new Map([["manifest.json", Buffer.from("{}")], ["图解.txt", Buffer.from("中文 空格")]]);
  const archive = makeZip(files), read = readStoredZip(archive);
  assert.deepEqual([...read].sort(), [...files].sort());
  const corrupt = Buffer.from(archive); corrupt[35] ^= 1;
  assert.throws(() => readStoredZip(corrupt));
  assert.throws(() => readStoredZip(makeZip(new Map([["a", Buffer.from("x")], ["A", Buffer.from("y")]]))));
  for (const name of ["../a","C:/a","/a","a\\b","a//b","CON.txt","a:stream"]) assert.equal(safePackageName(name), false);
});

// The bundler and compiler are fixtures; ZIP validation and evidence writes use
// production code. Real TypeScript/Vite builds run separately in frontend-build.
for (const development of [false, true]) {
  for (const failure of ["identity", "source", "typecheck", "bundle", "empty", "hash"]) {
    test(`${development ? "development" : "release"} extension failure invalidates old PASS: ${failure}`, async () => {
      const repository = mkdtempSync(join(tmpdir(), "localbridge-extension-build-regression-"));
      copyReleaseInputs(repository);
      const source = join(repository, "extensions/chatgpt-web");
      const evidence = join(repository, development ? "tests/artifacts/browser-extension-dev" : "tests/artifacts/browser-extension");
      const put = (path, bytes) => { mkdirSync(dirname(path), { recursive: true }); writeFileSync(path, bytes); };
      const suffix = development ? ".dev" : "";
      for (const name of ["identity" + suffix + ".json", "extension-id" + suffix + ".txt"]) put(join(source, name), readFileSync(new URL("../../extensions/chatgpt-web/" + name, import.meta.url)));
      for (const name of ["popup.html", "popup.css", "INSTALL.html", "INSTALL.svg"]) put(join(source, name), "fixture asset");
      put(join(repository, "package.json"), JSON.stringify({ version: "0.1.5" }));
      const evidencePath = join(evidence, "extension-build.json");
      const options = {
        repository, development, checkout: () => "a".repeat(40), worktree: () => false,
        typecheck: () => assert.equal(JSON.parse(readFileSync(evidencePath)).status, "BUILDING"),
        bundle: async (config) => put(join(config.build.outDir, config.build.lib.fileName()), "fixture compiled script"),
      };
      const old = await buildBrowserExtension(options);
      assert.equal(old.status, "PASS");
      if (!development) assert.equal(verifyBundledExtension(repository).extension.asset.sha256, old.sha256);
      assert.equal(verifyExtensionArchive(old.archive, repository, development).extensionId, old.extensionId);
      put(join(evidence, "user.txt"), "preserved");
      let checked = 0;
      const next = { ...options, typecheck: () => { checked++; options.typecheck(); } };
      if (failure === "identity") {
        const identity = JSON.parse(readFileSync(join(source, "identity" + suffix + ".json")));
        identity.id = "a".repeat(32); put(join(source, "identity" + suffix + ".json"), JSON.stringify(identity));
      }
      if (failure === "source") next.checkout = () => "unavailable";
      if (failure === "typecheck") next.typecheck = () => { options.typecheck(); throw new Error("fixture typecheck failed"); };
      if (failure === "bundle") next.bundle = async () => { throw new Error("fixture bundler failed"); };
      if (failure === "empty") next.bundle = async (config) => put(join(config.build.outDir, config.build.lib.fileName()), "");
      if (failure === "hash") next.verifyArchive = (archive, root, dev) => {
        const files = readStoredZip(readFileSync(archive));
        files.set("background.js", Buffer.from("tampered after metadata"));
        writeFileSync(archive, makeZip(files));
        return verifyExtensionArchive(archive, root, dev);
      };
      await assert.rejects(buildBrowserExtension(next));
      const failed = JSON.parse(readFileSync(evidencePath));
      assert.equal(failed.status, "FAIL");
      if (!development) assert.throws(() => verifyBundledExtension(repository));
      assert.equal(failed.development, development);
      assert.equal(failed.sha256, undefined);
      if (failure === "identity" || failure === "source") assert.equal(checked, 0);
      assert.equal(readFileSync(join(evidence, "user.txt"), "utf8"), "preserved");
      assert.equal(readFileSync(join(old.directory, "background.js"), "utf8"), "fixture compiled script");
    });
  }
}
test("fixed public key derives the committed extension ID", () => {
  const identity = JSON.parse(readFileSync(new URL("../../extensions/chatgpt-web/identity.json", import.meta.url), "utf8"));
  assert.equal(extensionId(identity.key), identity.id);
  assert.equal(readFileSync(new URL("../../extensions/chatgpt-web/extension-id.txt", import.meta.url), "utf8").trim(), identity.id);
});
test("development identity cannot match the shipped host or release identity", () => {
  const release = JSON.parse(readFileSync(new URL("../../extensions/chatgpt-web/identity.json", import.meta.url), "utf8"));
  const development = JSON.parse(readFileSync(new URL("../../extensions/chatgpt-web/identity.dev.json", import.meta.url), "utf8"));
  assert.equal(extensionId(development.key), development.id);
  assert.notEqual(development.id, release.id);
  assert.notEqual(development.host, release.host);
});
test("package metadata rejects tampering, missing entrypoints and a wrong protocol", () => {
  const identity = JSON.parse(readFileSync(new URL("../../extensions/chatgpt-web/identity.json", import.meta.url), "utf8"));
  const manifest = { manifest_version: 3, key: identity.key, version: "0.1.5", host_permissions: ["https://chatgpt.com/*"],
    permissions: ["nativeMessaging","storage","clipboardWrite"], background: { service_worker: "background.js" },
    action: { default_popup: "popup.html" }, content_scripts: [{ matches: ["https://chatgpt.com/*"], js: ["content.js"], all_frames: false }] };
  const files = new Map([["manifest.json",Buffer.from(JSON.stringify(manifest))], ...["background.js","popup.html","content.js","popup.js","popup.css","INSTALL.html","INSTALL.svg"].map(name => [name, Buffer.from("fixture")])]);
  const metadata = { schemaVersion: 1, protocol: 1, extensionId: identity.id, version: "0.1.5", application: ">=0.1.5, <0.2.0", files: Object.fromEntries([...files].map(([name,bytes]) => [name, sha256(bytes)])) };
  const stamp = () => files.set("localbridge-extension.json", Buffer.from(JSON.stringify(metadata)));
  stamp(); assert.equal(verifyExtensionFiles(files, identity).version, "0.1.5");
  files.set("background.js",Buffer.from("tampered")); assert.throws(() => verifyExtensionFiles(files,identity), /hash/);
  files.set("background.js",Buffer.from("fixture")); metadata.protocol=2; stamp(); assert.throws(() => verifyExtensionFiles(files,identity), /metadata/);
  metadata.protocol=1; metadata.files["background.js"] = sha256("fixture"); files.delete("background.js"); stamp(); assert.throws(() => verifyExtensionFiles(files,identity));
});

test("unchanged verified ZIP is reused, and source or staged corruption forces rebuilding", async () => {
  const repository = mkdtempSync(join(tmpdir(), "localbridge-extension-reuse-"));
  copyReleaseInputs(repository);
  for (const name of ["popup.html", "popup.css"]) writeFileSync(join(repository, "extensions/chatgpt-web", name), "fixture asset");
  let builds = 0;
  const options = { repository, checkout: () => "a".repeat(40), worktree: () => false,
    typecheck: () => { builds++; },
    bundle: async config => { mkdirSync(config.build.outDir, { recursive: true }); writeFileSync(join(config.build.outDir, config.build.lib.fileName()), "fixture compiled script"); } };
  const first = await ensureBrowserExtension(options);
  const reused = await ensureBrowserExtension(options);
  assert.equal(builds, 1); assert.equal(reused.sha256, first.sha256); assert.equal(reused.directory, first.directory);
  writeFileSync(join(repository, "extensions/chatgpt-web/popup.css"), "changed source");
  const changed = await ensureBrowserExtension(options);
  assert.equal(builds, 2); assert.notEqual(changed.sha256, first.sha256);
  writeFileSync(join(repository, "src-tauri/target/browser-extension-stage/extension.zip"), "corrupt");
  const repaired = await ensureBrowserExtension(options);
  assert.equal(builds, 3); assert.equal(verifyBundledExtension(repository).extension.asset.sha256, repaired.sha256);
});
