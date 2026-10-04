import { defineConfig } from "vitest/config";
import application from "../../package.json";
import release from "../../product-release.json";
export default defineConfig({
  define: { __LOCALBRIDGE_VERSION__: JSON.stringify(application.version), __LOCALBRIDGE_RELEASES_URL__: JSON.stringify(`https://github.com/${release.repository}/releases`) },
  test: { include: ["extensions/chatgpt-web/*.test.ts"] },
});
