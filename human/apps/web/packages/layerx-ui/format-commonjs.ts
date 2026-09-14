import ts from "typescript";
import { basename, resolve } from "node:path";
import { decode, encode } from "@jridgewell/sourcemap-codec";
import type { RawSourceMap } from "source-map";

export function formatCommonJs(code: string, filename: string): string {
  const source = ts.createSourceFile(
    filename, code, ts.ScriptTarget.Latest, false, ts.ScriptKind.JS,
  );
  const literals: [number, number][] = [];
  const visit = (node: ts.Node) => {
    if (ts.isStringLiteralLike(node) || ts.isTemplateLiteralToken(node)) {
      literals.push([node.getStart(source), node.getEnd()]);
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return code.replace(/[\t ]+(?=\r?$)/gmu, (spaces, offset: number) =>
    literals.some(([start, end]) => start <= offset && offset < end) ? spaces : "",
  );
}

export function formatCommonJsMap(
  encoded: string, before: string, after: string, filename: string,
): string {
  const map: RawSourceMap = JSON.parse(encoded);
  const originalLines = before.split("\n");
  const formattedLines = after.split("\n");
  const sourceName = (name: string) => name === resolve(filename) ? basename(filename) : name;
  map.mappings = encode(decode(map.mappings).map((line, index) =>
    line.filter((mapping) => {
      const originalLine = originalLines[index];
      const formattedLine = formattedLines[index];
      if (originalLine === undefined || formattedLine === undefined ||
          mapping[0] > originalLine.length) {
        throw new Error(`Invalid generated source-map location in ${filename}`);
      }
      return mapping[0] <= formattedLine.length;
    }),
  ));
  map.file = basename(filename);
  map.sources = map.sources.map(sourceName);
  return JSON.stringify(map);
}
