// Downloads the pinned Windows Codex binary that ships inside the installer.
// Source: the official npm package @openai/codex@<version>-win32-x64 (npm verifies its integrity).
// Usage: node scripts/fetch-codex.mjs   (version comes from package.json "codexVersion")
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const root = path.resolve(path.dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1")), "..");
const version = JSON.parse(fs.readFileSync(path.join(root, "package.json"), "utf8")).codexVersion;
if (!/^\d+\.\d+\.\d+$/.test(version ?? "")) throw new Error("package.json codexVersion is missing or invalid");

const outDir = path.join(root, "src-tauri", "resources", "codex");
const target = path.join(outDir, "codex.exe");
const stamp = path.join(outDir, "VERSION");
if (fs.existsSync(target) && fs.existsSync(stamp) && fs.readFileSync(stamp, "utf8").trim() === version) {
  console.log(`codex ${version} already present`);
  process.exit(0);
}

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "codex-win-"));
try {
  const npm = process.platform === "win32" ? "npm.cmd" : "npm";
  execFileSync(npm, ["pack", `@openai/codex@${version}-win32-x64`, "--pack-destination", tmp, "--silent"], {
    stdio: ["ignore", "ignore", "inherit"],
    shell: process.platform === "win32",
  });
  const tgz = fs.readdirSync(tmp).find((f) => f.endsWith(".tgz"));
  if (!tgz) throw new Error("npm pack produced no archive");
  const member = "package/vendor/x86_64-pc-windows-msvc/bin/codex.exe";
  execFileSync("tar", ["-xzf", path.join(tmp, tgz), "-C", tmp, member], { stdio: "inherit" });
  fs.mkdirSync(outDir, { recursive: true });
  fs.copyFileSync(path.join(tmp, member), target);
  fs.writeFileSync(stamp, version + "\n");
  const sha = createHash("sha256").update(fs.readFileSync(target)).digest("hex");
  console.log(`codex ${version} → ${path.relative(root, target)} (sha256 ${sha})`);
} finally {
  fs.rmSync(tmp, { recursive: true, force: true });
}
