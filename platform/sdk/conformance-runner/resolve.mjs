export async function resolve(specifier, context, nextResolve) {
  if (specifier === "@jest/globals") {
    return {
      shortCircuit: true,
      url: new URL(
        "../../../build/platform-sdk-conformance/platform/sdk/conformance-runner/node-test.js",
        import.meta.url,
      ).href,
    };
  }
  return nextResolve(specifier, context);
}
