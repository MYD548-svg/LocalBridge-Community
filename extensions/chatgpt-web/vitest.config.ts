import { defineConfig } from "vitest/config";
export default defineConfig({ test: { include: ["extensions/chatgpt-web/*.test.ts"] } });
