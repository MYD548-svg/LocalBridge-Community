import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

const buildEnvironment = (globalThis as typeof globalThis & { process?: { env: Record<string, string | undefined> } }).process?.env ?? {};

export default defineConfig({
  root: "src",
  plugins: [react()],
  server: {
    host: "127.0.0.1",
    port: 1420,
    strictPort: true,
  },
  build: {
    outDir: buildEnvironment.LOCALBRIDGE_FRONTEND_OUT_DIR || "../tests/artifacts/frontend-dist",
    emptyOutDir: false, // Retain outputs; repository policy forbids bulk cleanup.
  },
  test: {
    include: ["__tests__/**/*.test.{ts,tsx}"],
  },
});
