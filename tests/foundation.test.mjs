import test from "node:test";import assert from "node:assert/strict";import fs from "node:fs";
test("version matches in frontend and Tauri",()=>{const p=JSON.parse(fs.readFileSync("package.json","utf8"));const t=JSON.parse(fs.readFileSync("src-tauri/tauri.conf.json","utf8"));assert.equal(p.version,t.version)});
test("no privileged Tauri shell capabilities",()=>{const c=JSON.parse(fs.readFileSync("src-tauri/capabilities/default.json","utf8"));assert.ok(!JSON.stringify(c).includes("shell:allow-execute"))});
test("Codex methods use a strict whitelist",()=>{const b=fs.readFileSync("src-tauri/src/lib.rs","utf8");assert.ok(b.includes("validated_request"));assert.ok(b.includes("readOnly"));assert.ok(b.includes('account/login/start'));assert.ok(b.includes('turn/start'))});
