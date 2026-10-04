import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, readdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { applicationVersion, digest, readRequired, releaseConfiguration, releaseTag, verifyBundledExtension } from "./release-contract.mjs";
import { verifyExtensionArchive } from "./test/browser-extension-package.mjs";
import { CI_STAGE_IDS } from "./test/ci-contract.mjs";

export function stageReleaseCandidate(root, sourceCommit, installer) {
  const config = releaseConfiguration(root), version = applicationVersion(root), bundle = verifyBundledExtension(root);
  const host = JSON.parse(readRequired(join(root, "src-tauri/target/browser-host-stage/browser-host-build.json")));
  if (bundle.applicationVersion !== version || bundle.sourceCommit !== sourceCommit || host.status !== "PASS" || host.sourceSha !== sourceCommit
    || host.extensionId !== bundle.extension.extensionId || host.protocolVersion !== bundle.extension.protocol) throw new Error("installer and extension must share this source commit, identity and protocol");
  const installerName = installer.split(/[\\/]/).at(-1);
  if (installerName !== `LocalBridge_${version}_x64-setup.exe`) throw new Error("unexpected installer version or architecture");
  const directory = join(root, "tests/artifacts/release"); mkdirSync(directory, { recursive: true });
  const allowed = [installerName, bundle.extension.asset.name, config.manifestAsset, "INSTALL.html", "INSTALL.svg", "SHA256SUMS.txt"];
  if (readdirSync(directory).some(name => !allowed.includes(name))) throw new Error("unexpected release candidate files; preserve and remove stale files manually before retrying");
  const bytes = readRequired(installer);
  const manifest = { schemaVersion: 1, repository: config.repository, channel: config.channel, tag: releaseTag(config, version),
    sourceCommit, applicationVersion: version, installer: { name: installerName, size: bytes.length, sha256: digest(bytes) }, extension: bundle.extension };
  copyFileSync(installer, join(directory, installerName));
  copyFileSync(join(root, "src-tauri/target/browser-extension-stage/extension.zip"), join(directory, bundle.extension.asset.name));
  for (const name of ["INSTALL.html", "INSTALL.svg"]) copyFileSync(join(root, "extensions/chatgpt-web", name), join(directory, name));
  writeFileSync(join(directory, config.manifestAsset), JSON.stringify(manifest, null, 2) + "\n");
  const names = [installerName, bundle.extension.asset.name, config.manifestAsset, "INSTALL.html", "INSTALL.svg"];
  writeFileSync(join(directory, "SHA256SUMS.txt"), names.map(name => `${digest(readRequired(join(directory, name)))}  ${name}\n`).join(""));
  return manifest;
}

export function verifyReleaseCandidate(directory, root, sourceCommit) {
  const config = releaseConfiguration(root), version = applicationVersion(root);
  const release = join(directory, "release"), ci = join(directory, "ci");
  const manifest = JSON.parse(readRequired(join(release, config.manifestAsset)));
  const report = JSON.parse(readRequired(join(ci, "TEST-REPORT.json")));
  const provenance = JSON.parse(readRequired(join(ci, "BUILD-PROVENANCE.json")));
  const identity = JSON.parse(readRequired(join(root, "extensions/chatgpt-web/identity.json")));
  const stageIds = new Set(report.stages?.map(stage => stage.id));
  if (!/^[a-f0-9]{40}$/.test(sourceCommit ?? "") || manifest.schemaVersion !== 1 || manifest.repository !== config.repository
    || manifest.channel !== config.channel || manifest.applicationVersion !== version || manifest.tag !== releaseTag(config, version)
    || manifest.sourceCommit !== sourceCommit || report.commit !== sourceCommit || provenance.commit !== sourceCommit || provenance.profile !== "community"
    || report.status !== "PASS" || report.profile !== "community" || report.stages?.length !== CI_STAGE_IDS.length
    || stageIds.size !== CI_STAGE_IDS.length || !CI_STAGE_IDS.every(id => stageIds.has(id)) || report.stages.some(stage => stage.status !== "PASS")) {
    throw new Error("release source or complete community gate evidence mismatch");
  }
  if (manifest.installer?.name !== `LocalBridge_${version}_x64-setup.exe` || manifest.extension?.version !== version
    || manifest.extension.protocol !== identity.protocol || manifest.extension.extensionId !== identity.id
    || manifest.extension.application !== config.applicationCompatibility || manifest.extension.asset?.name !== `LocalBridge-ChatGPT-Web-v${version}.zip`) {
    throw new Error("release installer and extension are not a compatible pair");
  }
  const sums = readRequired(join(release, "SHA256SUMS.txt")).toString().trim().split(/\r?\n/);
  for (const asset of [manifest.installer, manifest.extension.asset]) {
    const bytes = readRequired(join(release, asset.name));
    const hash = digest(bytes);
    const original = asset === manifest.installer ? `src-tauri/target/release/bundle/nsis/${asset.name}` : `tests/artifacts/browser-extension/${asset.name}`;
    if (bytes.length !== asset.size || hash !== asset.sha256 || provenance.hashes?.[original] !== hash) throw new Error("release artifact hash or original provenance mismatch");
  }
  for (const name of [manifest.installer.name, manifest.extension.asset.name, config.manifestAsset, "INSTALL.html", "INSTALL.svg"]) {
    if (!sums.includes(`${digest(readRequired(join(release, name)))}  ${name}`)) throw new Error("release checksum list mismatch");
  }
  const extension = verifyExtensionArchive(join(release, manifest.extension.asset.name), root);
  if (extension.version !== version || extension.repository !== config.repository || extension.sourceCommit !== sourceCommit) throw new Error("release extension archive source mismatch");
  if (extension.files.LICENSE !== digest(readRequired(join(root, "LICENSE")))) throw new Error("extension license missing or changed");
  for (const name of ["INSTALL.html", "INSTALL.svg"]) {
    const hash = digest(readRequired(join(release, name)));
    if (extension.files[name] !== hash || hash !== digest(readRequired(join(root, "extensions/chatgpt-web", name)))) throw new Error("release installation guide differs from the extension");
  }
  return manifest;
}

function github(args) {
  const result = spawnSync("gh", args, { encoding: "utf8", windowsHide: true });
  if (result.status !== 0) throw new Error(`GitHub operation failed: ${result.stderr.trim()}`);
  return result.stdout.trim();
}
function compareVersions(left, right) {
  const a = left.split(".").map(Number), b = right.split(".").map(Number);
  for (let index = 0; index < 3; index++) if (a[index] !== b[index]) return a[index] - b[index];
  return 0;
}
export function publishRelease(directory, root, sourceCommit) {
  const manifest = verifyReleaseCandidate(directory, root, sourceCommit), config = releaseConfiguration(root);
  if (process.env.GITHUB_REPOSITORY !== config.repository || process.env.INPUT_RELEASE_TAG !== manifest.tag) throw new Error("publication repository or tag mismatch");
  const head = spawnSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" });
  if (head.status !== 0 || head.stdout.trim() !== sourceCommit) throw new Error("publication checkout must match the verified run");
  const latest = spawnSync("gh", ["api", `repos/${config.repository}/releases/latest`, "--jq", ".tag_name"], { encoding: "utf8" });
  if (latest.status === 0) {
    const previous = latest.stdout.trim().match(/^v(\d+\.\d+\.\d+)-community\.1$/)?.[1];
    if (!previous || compareVersions(manifest.applicationVersion, previous) <= 0) throw new Error("formal releases must increase the numeric application version");
  } else if (!latest.stderr.includes("HTTP 404")) throw new Error("cannot verify the current release; publication stopped");
  const release = join(directory, "release");
  const notes = join(directory, "RELEASE-NOTES.md");
  writeFileSync(notes, `LocalBridge Community ${manifest.applicationVersion}\n\n安装包已携带同次构建的 ChatGPT 网页扩展。安装后打开“设置 → ChatGPT 网页”，点击“准备配套扩展”，按向导在 Edge/Chrome 加载固定目录、批准配对并启用当前聊天。\n\n单独导入请下载 ${manifest.extension.asset.name}，不要下载 Source code ZIP。中文图解见 INSTALL.html / INSTALL.svg。\n\n来源提交：${sourceCommit}。完整社区构建与资源校验已通过；干净 Windows 安装和实际 ChatGPT 交互仍需按指引验收，不能由构建结果代替。\n`);
  const files = [manifest.installer.name, manifest.extension.asset.name, config.manifestAsset, "INSTALL.html", "INSTALL.svg", "SHA256SUMS.txt"].map(name => join(release, name));
  files.push(join(directory, "ci/TEST-REPORT.json"), join(directory, "ci/BUILD-PROVENANCE.json"));
  github(["release", "create", manifest.tag, ...files, "--repo", config.repository, "--verify-tag", "--latest", "--title", `LocalBridge Community ${manifest.applicationVersion}`, "--notes-file", notes]);
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [action, directory] = process.argv.slice(2);
  if (!["verify", "publish"].includes(action) || !directory || process.argv.length !== 4) throw new Error("usage: release-candidate.mjs <verify|publish> <downloaded artifact directory>");
  const root = resolve(import.meta.dirname, "..");
  const candidate = resolve(directory), source = process.env.VERIFIED_SOURCE_COMMIT;
  if (action === "publish") publishRelease(candidate, root, source);
  else { const manifest = verifyReleaseCandidate(candidate, root, source); console.log(`RELEASE_CANDIDATE=PASS ${manifest.tag} ${manifest.sourceCommit}`); }
}
