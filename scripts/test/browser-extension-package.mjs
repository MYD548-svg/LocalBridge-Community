import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { numericVersion, releaseConfiguration } from "../release-contract.mjs";
export const sha256 = bytes => createHash("sha256").update(bytes).digest("hex");
export function extensionId(key) {
  return Array.from(createHash("sha256").update(Buffer.from(key, "base64")).digest().subarray(0, 16))
    .flatMap(byte => [byte >> 4, byte & 15]).map(value => String.fromCharCode(97 + value)).join("");
}
function crc32(bytes) {
  let crc = 0xffffffff;
  for (const byte of bytes) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit++) crc = (crc >>> 1) ^ ((crc & 1) ? 0xedb88320 : 0);
  }
  return (crc ^ 0xffffffff) >>> 0;
}
export function safePackageName(name) {
  return typeof name === "string" && name.length > 0 && name.length < 180 && !/[\\:<>"|*?\x00-\x1f]/.test(name)
    && name.split("/").every(part => part && part !== "." && part !== ".." && !/[ .]$/.test(part)
      && !/^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\.|$)/i.test(part));
}
// A deterministic stored ZIP needs no extra runtime dependency or 7-Zip.
export function makeZip(files) {
  const locals = [], central = []; let offset = 0;
  for (const [name, input] of [...files].sort(([a], [b]) => a.localeCompare(b))) {
    if (!safePackageName(name)) throw new Error("unsafe ZIP entry");
    const bytes = Buffer.from(input), encoded = Buffer.from(name), crc = crc32(bytes);
    const header = Buffer.alloc(30);
    header.writeUInt32LE(0x04034b50); header.writeUInt16LE(20, 4); header.writeUInt16LE(0x800, 6);
    header.writeUInt16LE(0x21, 12); header.writeUInt32LE(crc, 14);
    header.writeUInt32LE(bytes.length, 18); header.writeUInt32LE(bytes.length, 22); header.writeUInt16LE(encoded.length, 26);
    const directory = Buffer.alloc(46);
    directory.writeUInt32LE(0x02014b50); directory.writeUInt16LE(20, 4); directory.writeUInt16LE(20, 6);
    directory.writeUInt16LE(0x800, 8); directory.writeUInt16LE(0x21, 14); directory.writeUInt32LE(crc, 16);
    directory.writeUInt32LE(bytes.length, 20); directory.writeUInt32LE(bytes.length, 24);
    directory.writeUInt16LE(encoded.length, 28); directory.writeUInt32LE(offset, 42);
    locals.push(header, encoded, bytes); central.push(directory, encoded);
    offset += header.length + encoded.length + bytes.length;
  }
  const directory = Buffer.concat(central), end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50); end.writeUInt16LE(files.size, 8); end.writeUInt16LE(files.size, 10);
  end.writeUInt32LE(directory.length, 12); end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, directory, end]);
}
export function readStoredZip(archive) {
  if (archive.length > 32 * 1024 * 1024) throw new Error("ZIP exceeds limit");
  const files = new Map(), locations = new Map(); let offset = 0, expanded = 0;
  while (offset + 4 <= archive.length && archive.readUInt32LE(offset) === 0x04034b50) {
    if (offset + 30 > archive.length) throw new Error("truncated ZIP");
    const flags = archive.readUInt16LE(offset + 6), method = archive.readUInt16LE(offset + 8);
    const size = archive.readUInt32LE(offset + 18), plain = archive.readUInt32LE(offset + 22);
    const nameLength = archive.readUInt16LE(offset + 26), extra = archive.readUInt16LE(offset + 28);
    const name = archive.subarray(offset + 30, offset + 30 + nameLength).toString("utf8");
    const start = offset + 30 + nameLength + extra, end = start + size;
    if (flags !== 0x800 || method !== 0 || size !== plain || end > archive.length || !safePackageName(name)) throw new Error("invalid release ZIP");
    expanded += size;
    if (expanded > 32 * 1024 * 1024 || files.size >= 65 || [...files.keys()].some(key => key.toLowerCase() === name.toLowerCase())) throw new Error("ZIP capacity or duplicate");
    const bytes = archive.subarray(start, end);
    if (crc32(bytes) !== archive.readUInt32LE(offset + 14)) throw new Error("ZIP CRC mismatch");
    files.set(name, bytes); locations.set(name, offset); offset = end;
  }
  if (offset + 4 > archive.length || archive.readUInt32LE(offset) !== 0x02014b50) throw new Error("ZIP central directory missing");
  const centralStart = offset; let count = 0;
  while (offset + 46 <= archive.length && archive.readUInt32LE(offset) === 0x02014b50) {
    const nameLength = archive.readUInt16LE(offset + 28), extra = archive.readUInt16LE(offset + 30), comment = archive.readUInt16LE(offset + 32);
    const name = archive.subarray(offset + 46, offset + 46 + nameLength).toString("utf8");
    const local = locations.get(name);
    if (local === undefined || archive.readUInt32LE(offset + 42) !== local
      || archive.readUInt32LE(offset + 16) !== crc32(files.get(name)) || archive.readUInt32LE(offset + 24) !== files.get(name).length) throw new Error("ZIP central identity mismatch");
    locations.delete(name); offset += 46 + nameLength + extra + comment; count++;
  }
  if (locations.size || count !== files.size || offset + 22 !== archive.length || archive.readUInt32LE(offset) !== 0x06054b50
    || archive.readUInt16LE(offset + 10) !== count || archive.readUInt32LE(offset + 12) !== offset - centralStart
    || archive.readUInt32LE(offset + 16) !== centralStart) throw new Error("ZIP end directory mismatch");
  return files;
}
export function verifyExtensionFiles(files, identity, config = releaseConfiguration(resolve(import.meta.dirname, "../.."))) {
  const metadata = JSON.parse(files.get("localbridge-extension.json") || "null");
  const manifest = JSON.parse(files.get("manifest.json") || "null");
  if (!metadata || metadata.schemaVersion !== 1 || metadata.protocol !== 1 || metadata.extensionId !== identity.id
    || extensionId(identity.key) !== identity.id || metadata.version !== manifest?.version || manifest?.manifest_version !== 3
    || manifest?.key !== identity.key || metadata.application !== config.applicationCompatibility
    || !numericVersion(metadata.version) || (metadata.repository !== undefined && metadata.repository !== config.repository)
    || (metadata.sourceCommit !== undefined && !/^[a-f0-9]{40}$/.test(metadata.sourceCommit)) || files.size !== Object.keys(metadata.files || {}).length + 1) throw new Error("extension identity or metadata mismatch");
  if (JSON.stringify(manifest.content_scripts?.[0]?.matches) !== JSON.stringify(["https://chatgpt.com/*"])
    || manifest.externally_connectable || JSON.stringify(manifest.host_permissions) !== JSON.stringify(["https://chatgpt.com/*"])
    || JSON.stringify(manifest.permissions) !== JSON.stringify(["nativeMessaging","storage","clipboardWrite"])
    || manifest.content_scripts.length !== 1 || manifest.content_scripts[0].all_frames !== false) throw new Error("extension permission mismatch");
  for (const [name, expected] of Object.entries(metadata.files)) {
    if (!safePackageName(name) || !files.has(name) || sha256(files.get(name)) !== expected) throw new Error("extension file hash mismatch: " + name);
  }
  for (const entry of [manifest.background?.service_worker, manifest.action?.default_popup, ...manifest.content_scripts.flatMap(script => script.js)]) {
    if (!files.has(entry)) throw new Error("missing manifest entry point: " + entry);
  }
  for (const entry of ["popup.js", "popup.css", "INSTALL.html", "INSTALL.svg"]) if (!files.has(entry)) throw new Error("missing UI asset: " + entry);
  return metadata;
}
export function verifyExtensionArchive(path, root = resolve(import.meta.dirname, "../.."), development = false) {
  return verifyExtensionFiles(readStoredZip(readFileSync(path)),
    JSON.parse(readFileSync(resolve(root, development ? "extensions/chatgpt-web/identity.dev.json" : "extensions/chatgpt-web/identity.json"), "utf8")), releaseConfiguration(root));
}
