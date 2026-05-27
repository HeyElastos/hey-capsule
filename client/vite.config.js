import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Vidstack 1.x ships JSX inside the .js files in its prod bundle. Both Vite's
// CJS pre-resolver and Rollup's parser choke on the raw JSX, and the React
// plugin won't touch node_modules. Pre-transform Vidstack's files with esbuild
// (jsx loader) so Rollup only ever sees plain JS.
const vidstackJsxLoader = () => ({
  name: "vidstack-jsx-loader",
  enforce: "pre",
  async transform(code, id) {
    if (!id.includes("/node_modules/@vidstack/react/") || !id.endsWith(".js")) {
      return null;
    }
    const esbuild = await import("esbuild");
    const result = await esbuild.transform(code, {
      loader: "jsx",
      jsx: "automatic",
      target: "es2020",
      sourcefile: id,
      sourcemap: true,
    });
    return { code: result.code, map: result.map };
  },
});

export default defineConfig({
  // Relative asset references so the bundle works under any mount path.
  // When the runtime serves the capsule at /elastos/apps/hey-social/ on
  // YunoHost (or /apps/hey-social/ at root), the index.html's
  // ./assets/index-*.js stays correct without needing a hardcoded base.
  // Without this, Vite emits /assets/... (root-absolute), which under
  // YunoHost's subpath mount hits the wrong nginx location → SSOwat
  // 302 → MIME-type mismatch → blank white React iframe.
  base: "./",
  plugins: [vidstackJsxLoader(), react()],
  optimizeDeps: {
    include: [
      "@vidstack/react",
      "@vidstack/react/icons",
      "@vidstack/react/player/layouts/default",
    ],
  },
  server: {
    port: 3000,
    proxy: {
      "/api": {
        target: "http://127.0.0.1:4000",
        changeOrigin: true,
        rewrite: (path) => path.replace(/^\/api/, ""),
      },
      "/uploads": {
        target: "http://127.0.0.1:4000",
        changeOrigin: true,
      },
    },
  },
});
