import { defineConfig, type Plugin } from "tsup";
import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { formatCommonJs, formatCommonJsMap } from "./format-commonjs";

const commonJsWhitespace: Plugin = {
  name: "commonjs-whitespace",
  buildEnd({ writtenFiles }) {
    for (const file of writtenFiles) {
      if (!file.name.endsWith(".cjs")) continue;
      const code = readFileSync(file.name, "utf8");
      const formatted = formatCommonJs(code, file.name);
      if (formatted !== code) {
        const mapPath = `${file.name}.map`;
        const map = formatCommonJsMap(readFileSync(mapPath, "utf8"), code, formatted, file.name);
        writeFileSync(file.name, formatted);
        writeFileSync(mapPath, map);
      }
    }
  },
};

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
  plugins: [commonJsWhitespace],
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
