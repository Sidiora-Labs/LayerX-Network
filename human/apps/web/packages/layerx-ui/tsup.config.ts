import { defineConfig } from "tsup";
import { readdirSync } from "node:fs";

const componentEntries = Object.fromEntries(
  ["components", "lib"].flatMap((directory) =>
    readdirSync(`src/${directory}`)
      .filter((name) => /\.tsx?$/u.test(name))
      .map((name) => [`${directory}/${name.replace(/\.tsx?$/u, "")}`, `src/${directory}/${name}`]),
  ),
);

const shared = {
  format: ["esm", "cjs"] as const,
  dts: true,
  sourcemap: true,
  clean: false,
  external: ["react", "react-dom", "tailwindcss"],
  esbuildOptions(options: { jsx: string }) {
    options.jsx = "automatic";
  },
};

export default defineConfig([
  {
    ...shared,
    entry: { index: "src/index.ts", ...componentEntries },
    splitting: true,
    banner: { js: '"use client";' },
  },
  {
    ...shared,
    entry: { cn: "src/lib/utils.ts" },
  },
]);
