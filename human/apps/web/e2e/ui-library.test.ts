import assert from "node:assert/strict";
import test from "node:test";
import { runInNewContext } from "node:vm";
import { SourceMapConsumer, SourceMapGenerator } from "source-map";

import { human_design_tokens } from "../src/design/tokens.ts";
import { formatCommonJs, formatCommonJsMap } from "../packages/layerx-ui/format-commonjs.ts";

test("the owner-supplied LayerX UI package is the visual contract", () => {
  const contract = human_design_tokens();
  assert.equal(contract.package, "@layerx/ui");
  assert.equal(contract.stylesheet, "@layerx/ui/styles.css");
  assert.ok(contract.styleFeatures.includes("borders"));
  assert.ok(contract.styleFeatures.includes("shadows"));
  assert.ok(contract.tokens.includes("--border"));
  assert.ok(contract.tokens.includes("--shadow-overlay"));
});

test("CommonJS formatting preserves token mappings and removes only deleted whitespace mappings", async () => {
  const source = "const value = 1;  \n";
  const formatted = formatCommonJs(source, "output.cjs");
  const map = new SourceMapGenerator({ file: "output.cjs" });
  map.addMapping({ generated: { line: 1, column: 6 },
    source: "input.ts", original: { line: 7, column: 8 }, name: "value" });
  map.addMapping({ generated: { line: 1, column: 18 },
    source: "input.ts", original: { line: 7, column: 20 } });
  const rawMap = JSON.parse(map.toString());
  rawMap.mappings = `A,${rawMap.mappings}`;
  const result = formatCommonJsMap(JSON.stringify(rawMap), source, formatted, "output.cjs");
  await SourceMapConsumer.with(result, null, (consumer) => {
    assert.deepEqual(consumer.originalPositionFor({ line: 1, column: 0 }),
      { source: null, line: null, column: null, name: null });
    assert.deepEqual(consumer.originalPositionFor({ line: 1, column: 6 }),
      { source: "input.ts", line: 7, column: 8, name: "value" });
    const columns: number[] = [];
    consumer.eachMapping((mapping) => columns.push(mapping.generatedColumn));
    assert.deepEqual(columns, [0, 6]);
  });
  map.addMapping({ generated: { line: 2, column: 1 },
    source: "input.ts", original: { line: 8, column: 1 } });
  assert.throws(() => formatCommonJsMap(map.toString(), source, formatted, "output.cjs"),
    /Invalid generated source-map location/u);
});

test("CommonJS formatting preserves literal whitespace and line positions", () => {
  const source = [
    'const word = "kept";  ',
    'const plain = `first  ',
    'second\t ',
    '${word}  ',
    '`; \t',
    'const tagged = String.raw`raw\\n  ',
    '${`nested  ',
    'value\t `}  ',
    '`;  ',
    'globalThis.result = { plain, tagged };  ',
    '// retained comment  ',
    '//# sourceMappingURL=output.cjs.map  ',
  ].join("\n");
  const formatted = formatCommonJs(source, "output.cjs");
  assert.equal(runInNewContext(`${formatted}\nJSON.stringify(result)`),
    runInNewContext(`${source}\nJSON.stringify(result)`));
  assert.equal(formatted.split("\n").length, source.split("\n").length);
  assert.equal(formatCommonJs(formatted, "output.cjs"), formatted);
  assert.ok(formatted.includes('const word = "kept";\n'));
  assert.ok(formatted.includes('first  \nsecond\t \n'));
  assert.ok(formatted.endsWith('//# sourceMappingURL=output.cjs.map'));
});
