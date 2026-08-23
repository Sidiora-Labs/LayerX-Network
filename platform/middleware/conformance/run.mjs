import { runScenarios } from "./dist/scenarios.js";
import { runServiceScenarios } from "./service.mjs";

// The middleware conformance runner. It exercises the buyer and seller
// middleware — in process and against the running paid-api service — and
// enforces the non-authority rule: no code path may present success without
// backing evidence. Any failing check exits non-zero so CI fails hard.
async function main() {
  const suite = await runScenarios();
  await runServiceScenarios(suite);
  const results = suite.results();
  let failures = 0;
  for (const result of results) {
    if (result.ok) {
      process.stdout.write(`ok    ${result.name}\n`);
    } else {
      failures += 1;
      process.stdout.write(`FAIL  ${result.name}\n      ${result.detail ?? "no detail"}\n`);
    }
  }
  process.stdout.write(`\n${results.length - failures}/${results.length} conformance checks passed\n`);
  return failures === 0 ? 0 : 1;
}

main()
  .then((code) => {
    process.exitCode = code;
  })
  .catch((error) => {
    process.stderr.write(`conformance runner crashed: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`);
    process.exitCode = 1;
  });
