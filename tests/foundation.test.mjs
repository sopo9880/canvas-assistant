import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";

const read = (p) => fs.readFileSync(p, "utf8");

test("version matches across package.json, tauri.conf.json and Cargo.toml", () => {
  const pkg = JSON.parse(read("package.json")).version;
  const tauri = JSON.parse(read("src-tauri/tauri.conf.json")).version;
  const cargo = read("src-tauri/Cargo.toml").match(/^version = "([^"]+)"/m)[1];
  assert.equal(pkg, tauri);
  assert.equal(pkg, cargo);
});

test("window capabilities stay minimal (no shell, fs or http plugins)", () => {
  const cap = JSON.stringify(JSON.parse(read("src-tauri/capabilities/default.json")));
  for (const forbidden of ["shell:", "fs:", "http:", "dialog:", "process:"]) assert.ok(!cap.includes(forbidden), forbidden);
});

test("a content security policy is configured", () => {
  const csp = JSON.parse(read("src-tauri/tauri.conf.json")).app.security.csp;
  assert.ok(typeof csp === "string" && csp.includes("default-src 'self'"));
});

test("Codex bridge forces read-only threads with a method whitelist", () => {
  const full = read("src-tauri/src/codex.rs");
  const src = full.slice(0, full.indexOf("#[cfg(test)]"));
  assert.ok(src.includes("fn validated_request"));
  assert.ok(src.includes('"sandbox": "read-only"'));
  assert.ok(src.includes('"approvalPolicy": "never"'));
  assert.ok(!src.includes('"danger-full-access"'));
});

test("Canvas writes only happen through the approval queue", () => {
  const mcp = read("src-tauri/src/mcp.rs");
  assert.ok(!mcp.includes("execute_write"), "the MCP helper must never execute writes");
  const lib = read("src-tauri/src/lib.rs");
  assert.ok(lib.includes("claim_pending") && lib.includes("execute_write"));
});

test("the access token is never persisted to the settings file", () => {
  const cfg = read("src-tauri/src/config.rs");
  const settingsStruct = cfg.slice(cfg.indexOf("pub struct Settings"), cfg.indexOf("impl Default for Settings"));
  assert.ok(!/token/i.test(settingsStruct));
});

test("app icons generated from the logo are committed", () => {
  for (const f of ["32x32.png", "128x128.png", "128x128@2x.png", "icon.ico", "icon.png"]) {
    assert.ok(fs.existsSync(`src-tauri/icons/${f}`), f);
  }
});

test("the installer bundles a pinned Codex and the bridge locks it down", () => {
  const pkg = JSON.parse(read("package.json"));
  assert.match(pkg.codexVersion, /^\d+\.\d+\.\d+$/);
  const res = JSON.parse(read("src-tauri/tauri.conf.json")).bundle.resources;
  assert.equal(res["resources/codex/codex.exe"], "codex/codex.exe");
  const src = read("src-tauri/src/codex.rs");
  for (const flag of ['cli_auth_credentials_store=\\"keyring\\"', "features.shell_tool=false", 'web_search=\\"disabled\\"', '"CODEX_HOME"']) {
    assert.ok(src.includes(flag), flag);
  }
});

test("links open through the validated open_external command, not a WebView permission", () => {
  const cap = JSON.stringify(JSON.parse(read("src-tauri/capabilities/default.json")));
  assert.ok(!cap.includes("opener:"), "the WebView gets no opener permission");
  const lib = read("src-tauri/src/lib.rs");
  assert.ok(lib.includes("fn open_external") && lib.includes('matches!(parsed.scheme(), "https" | "http")'));
  assert.ok(read("src/components.tsx").includes('invoke("open_external"'));
  for (const f of ["src/App.tsx", "src/components.tsx", "src/CodexLogin.tsx"]) {
    assert.ok(!read(f).includes("plugin-opener"), f);
  }
});

test("AI proposals cannot carry local file paths and user-only actions are not proposable", () => {
  const mcp = read("src-tauri/src/mcp.rs");
  assert.ok(mcp.includes('obj.remove("files")'));
  assert.ok(mcp.includes('"what_if_reset" | "conversation_read"'));
});

test("the generic Canvas API tool is read-only and path-guarded", () => {
  const ext = read("src-tauri/src/canvas_ext.rs");
  assert.ok(ext.includes("pub fn safe_api_path") && ext.includes('p.contains("..")'));
  const apiGet = ext.slice(ext.indexOf("pub fn api_get"), ext.indexOf("// ---------------- file upload"));
  assert.ok(apiGet.includes("get_all") && !apiGet.includes("post_form") && !apiGet.includes("put_form"));
});

test("storage uploads never carry the Canvas token", () => {
  const ext = read("src-tauri/src/canvas_ext.rs");
  const upload = ext.slice(ext.indexOf("pub fn upload_file"), ext.indexOf("// ---------------- writes"));
  const storagePost = upload.slice(upload.indexOf("storage.post"), upload.indexOf(".send(&body"));
  assert.ok(!storagePost.includes("Authorization"));
});
