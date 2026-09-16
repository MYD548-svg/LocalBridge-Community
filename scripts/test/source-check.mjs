import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
function git(args) {
  const result = spawnSync("git", args, { encoding: "utf8", windowsHide: true });
  if (result.status !== 0) throw new Error(`cannot verify source checkout: ${result.stderr ?? result.error}`);
  return result.stdout.trim();
}
export function validateCheckout(directory, commit, execute = git) {
  if (!/^[a-f0-9]{40}$/.test(commit)) throw new Error("full pinned commit required");
  const root = resolve(directory);
  const actualRoot = resolve(execute(["-C", root, "rev-parse", "--show-toplevel"]));
  if (actualRoot.toLowerCase() !== root.toLowerCase()) throw new Error("WorkDir must be the exact source repository root");
  if (execute(["-C", root, "rev-parse", "HEAD"]) !== commit) throw new Error("source commit mismatch");
  if (execute(["-C", root, "status", "--porcelain", "--untracked-files=all"])) throw new Error("source checkout must be clean");
  return root;
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) console.log(validateCheckout(...process.argv.slice(2)));
