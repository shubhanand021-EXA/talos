import { defineConfig } from "tsup";

export default defineConfig({
  entry: ["src/index.ts", "src/worker.ts"],
  format: ["esm"],
  dts: true,
  sourcemap: true,
  clean: true,
  treeshake: true,
  target: "es2022",
  // Playwright is an optional peer (live-only): never bundle it, load it
  // dynamically at runtime so CI and mock installs stay lean.
  external: ["playwright"],
  // No code-splitting: each entry must be a self-contained executable so the
  // worker CLI's `import.meta.url` entrypoint check actually fires.
  splitting: false,
});
