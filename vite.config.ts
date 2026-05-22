import path from "node:path";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vitest/config";

const host = process.env.TAURI_DEV_HOST;

// Playwright sets VITE_E2E=true on its webServer env so the IPC wrappers
// at `src/lib/` get swapped for the static mocks under `tests/e2e/mocks/`.
// Keeps production source pure (no test branches), keeps `pnpm tauri dev` /
// `pnpm tauri build` / Vitest untouched (none set this var).
const e2e = process.env.VITE_E2E === "true";
const e2eAliases: Record<string, string> = e2e
  ? {
      "@/lib/tools/pdfToImages": path.resolve(
        import.meta.dirname,
        "./tests/e2e/mocks/pdfToImages.ts",
      ),
      "@/lib/tools/imagesToPdf": path.resolve(
        import.meta.dirname,
        "./tests/e2e/mocks/imagesToPdf.ts",
      ),
      "@/lib/tools/imageFormatConverter": path.resolve(
        import.meta.dirname,
        "./tests/e2e/mocks/imageFormatConverter.ts",
      ),
      "@/lib/system": path.resolve(
        import.meta.dirname,
        "./tests/e2e/mocks/system.ts",
      ),
    }
  : {};

// https://vite.dev/config/
export default defineConfig({
  plugins: [react(), tailwindcss()],

  resolve: {
    alias: {
      ...e2eAliases,
      "@": path.resolve(import.meta.dirname, "./src"),
    },
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host ?? false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },

  test: {
    environment: "jsdom",
    setupFiles: ["./tests/setup.ts"],
    include: ["src/**/*.{test,spec}.{ts,tsx}"],
    exclude: ["tests/e2e/**", "node_modules", "dist", "src-tauri"],
    coverage: {
      provider: "v8",
      reporter: ["text", "html", "lcov"],
      include: ["src/**/*.{ts,tsx}"],
      exclude: [
        "src/**/*.{test,spec}.{ts,tsx}",
        "src/**/*.d.ts",
        "src/main.tsx",
        "src/vite-env.d.ts",
      ],
    },
  },
});
