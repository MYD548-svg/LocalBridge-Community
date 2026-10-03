import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { extensionId, makeZip, readStoredZip, safePackageName, sha256, verifyExtensionFiles } from "./browser-extension-package.mjs";
test("stored release ZIP roundtrips Unicode and rejects corruption and aliases", () => {
  const files = new Map([["manifest.json", Buffer.from("{}")], ["图解.txt", Buffer.from("中文 空格")]]);
  const archive = makeZip(files), read = readStoredZip(archive);
  assert.deepEqual([...read].sort(), [...files].sort());
  const corrupt = Buffer.from(archive); corrupt[35] ^= 1;
  assert.throws(() => readStoredZip(corrupt));
  assert.throws(() => readStoredZip(makeZip(new Map([["a", Buffer.from("x")], ["A", Buffer.from("y")]]))));
  for (const name of ["../a","C:/a","/a","a\\b","a//b","CON.txt","a:stream"]) assert.equal(safePackageName(name), false);
});
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
