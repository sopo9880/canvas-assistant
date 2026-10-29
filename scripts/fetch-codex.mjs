// Downloads the pinned Codex binaries that ship inside the app:
// codex and codex-code-mode-host, which Codex starts from the same folder whenever the
// model calls tools through Code Mode (without it every model tool call fails).
// Source: the official npm package @openai/codex@<version>-<platform> (npm verifies its integrity).
//
// Usage: node scripts/fetch-codex.mjs [--target win32-x64|darwin-arm64|darwin-x64]
// Default target: this machine's platform on Windows/macOS, win32-x64 elsewhere (cross builds).
// Version comes from package.json "codexVersion".
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const TARGETS = {
  "win32-x64": { triple: "x86_64-pc-windows-msvc", files: ["codex.exe", "codex-code-mode-host.exe"] },
  "darwin-arm64": { triple: "aarch64-apple-darwin", files: ["codex", "codex-code-mode-host"] },
  "darwin-x64": { triple: "x86_64-apple-darwin", files: ["codex", "codex-code-mode-host"] },
};

const root = path.resolve(path.dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1")), "..");
const version = JSON.parse(fs.readFileSync(path.join(root, "package.json"), "utf8")).codexVersion;
if (!/^\d+\.\d+\.\d+$/.test(version ?? "")) throw new Error("package.json codexVersion is missing or invalid");

const argIndex = process.argv.indexOf("--target");
const target = (argIndex > 0 ? process.argv[argIndex + 1] : process.env.CODEX_TARGET)
  || (process.platform === "darwin" ? `darwin-${process.arch === "arm64" ? "arm64" : "x64"}` : "win32-x64");
const spec = TARGETS[target];
if (!spec) throw new Error(`unknown target ${target} (use ${Object.keys(TARGETS).join(", ")})`);

const outDir = path.join(root, "src-tauri", "resources", "codex");
const stamp = path.join(outDir, "VERSION");
const wanted = `${version} ${target}`;
if (spec.files.every((f) => fs.existsSync(path.join(outDir, f))) && fs.existsSync(stamp) && fs.readFileSync(stamp, "utf8").trim() === wanted) {
  console.log(`codex ${wanted} already present`);
  process.exit(0);
}

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "codex-pkg-"));
try {
  const npm = process.platform === "win32" ? "npm.cmd" : "npm";
  execFileSync(npm, ["pack", `@openai/codex@${version}-${target}`, "--pack-destination", tmp, "--silent"], {
    stdio: ["ignore", "ignore", "inherit"],
    shell: process.platform === "win32",
  });
  const tgz = fs.readdirSync(tmp).find((f) => f.endsWith(".tgz"));
  if (!tgz) throw new Error("npm pack produced no archive");
  const members = spec.files.map((f) => `package/vendor/${spec.triple}/bin/${f}`);
  execFileSync("tar", ["-xzf", path.join(tmp, tgz), "-C", tmp, ...members], { stdio: "inherit" });
  // Only one platform's binaries live here at a time.
  fs.rmSync(outDir, { recursive: true, force: true });
  fs.mkdirSync(outDir, { recursive: true });
  for (const [i, f] of spec.files.entries()) {
    const dest = path.join(outDir, f);
    fs.copyFileSync(path.join(tmp, members[i]), dest);
    fs.chmodSync(dest, 0o755);
    const sha = createHash("sha256").update(fs.readFileSync(dest)).digest("hex");
    console.log(`codex ${wanted} → ${path.relative(root, dest)} (sha256 ${sha})`);
  }
  fs.writeFileSync(stamp, wanted + "\n");
} finally {
  fs.rmSync(tmp, { recursive: true, force: true });
}
