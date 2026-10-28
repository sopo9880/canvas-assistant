// Dev-only: renders the UI in a normal browser with a mocked Tauri backend.
// Run with `npm run ui:preview` and open http://127.0.0.1:5199/?view=chat|canvas|approvals|automation|settings
import { defineConfig } from "vite";
export default defineConfig({
  root: __dirname,
  publicDir: "../public",
  server: { port: 5199, strictPort: true, host: "127.0.0.1" },
});
