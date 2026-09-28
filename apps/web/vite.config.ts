import { readFileSync } from "node:fs";
import { fileURLToPath, URL } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// The version is the Cargo workspace's, which each release tag moves: one
// number for the core and the app, shown in the header.
const cargo = readFileSync(new URL("../../Cargo.toml", import.meta.url), "utf8");
const version = /^version\s*=\s*"([^"]+)"/m.exec(cargo)?.[1] ?? "0.0.0";

// GitHub Pages serves the app from /<repo-name>/, so the base path must match
// the repository name. GG_BASE is set by CI; local dev stays at "/".
export default defineConfig({
  base: process.env.GG_BASE ?? "/",
  define: { __APP_VERSION__: JSON.stringify(version) },
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  server: { fs: { allow: [".."] } },
});
