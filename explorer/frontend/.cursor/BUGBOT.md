# Bugbot PR Review Rules

In addition to the regular bug-finding process, check the following aspects in the code changes introduced by the Pull Request.

## 1. Design System & Theming

- Verify that all color values use **semantic color tokens** from the project's design system (`text.secondary`, `border.divider`, `icon.secondary`, `button.solid.bg`, `global.body.bg`, etc.). Flag any hardcoded color values such as `rgb(...)`, `#hex`. The full list of available tokens can be found in `toolkit/theme/foundations/semanticTokens.ts` and `toolkit/theme/foundations/colors.ts`. In rare cases the author may use hardcoded color values, but they must leave a comment explaining why.
- Flag any custom `box-shadow` values. Advise the author to use the design system's shadow tokens instead.
- Check for custom margins or paddings that override defaults of **internal parts** of compound components (e.g., `DialogHeader` inside a `Dialog`). This rule does not apply to the root component itself. If a part component already has default spacing from the design system, it should not be overridden with custom values. This applies to all components from the toolkit (see `toolkit/chakra`).
- Look for duplicated style props — check whether a property is already set by an inherited style prop and flag any redundant re-declarations.
- Refer to the Chakra documentation at [llms.txt](https://chakra-ui.com/llms.txt) to understand what components Chakra UI provides and how the styling system works.

## 2. TypeScript & Code Quality

- Check for usage of the `any` type. If found, advise the author to use `unknown` instead and narrow it properly in the code.
- Look for `eslint-disable` comments. If any exist without an accompanying explanation, ask the author to add a comment describing why the rule was disabled. Example: `// eslint-disable-next-line @typescript-eslint/no-explicit-any -- API response shape is dynamic and validated at runtime`.
- Identify magic numbers in the code. Advise the author to extract them into named constants using `UPPER_UNDERSCORE_CASE` and place the constants above the component definition. For example, `const MAX_VISIBLE_ITEMS = 4;` is preferable to an inline `4`.
- Evaluate naming of hooks, features, components, functions, and variables. Names should be specific and self-documenting. Flag vague names like `useWidgets` and suggest more descriptive alternatives such as `useAddress3rdPartyWidgets`.
- Where an `as MyType[]` assertion is used, suggest using `satisfies Array<MyType>` instead, if applicable.
- Check for inline empty arrays or objects used as default values on every render (e.g., `?? []`). Advise the author to define a static constant outside the component (e.g., `const EMPTY_ARRAY: Array<T> = [];`) and reference it instead.
- Check whether `.filter()`, `.map()`, or `.reduce()` calls produce new array references that are passed as props or used as hook dependencies. If so, advise the author to wrap them in `useMemo`.
- Flag unnecessary string interpolation in file paths. Explicit file names (e.g., `streak_30.png`) are preferable to template literals (e.g., `` `streak_${days}.png` ``), as they are easier to locate with search tools.

## 3. Environment Variables

When a PR adds, changes, renames, or removes an environment variable, verify that **all** of the following steps have been completed:

- The variable is documented in `docs/ENVS.md` with its name, expected type, compulsoriness, default value, and an example value.
- The variable is added to the app config in the appropriate section of `configs/app/` (`features/`, `ui.ts`, `api.ts`, etc.).
- A validation schema is added or updated in `deploy/tools/envs-validator/schema.ts`. For complex configs, a **separate sub-schema** should be created (similar to `tacScheme` or `beaconChainSchema`).
- The variable is added to the test presets in `deploy/tools/envs-validator/test/.env.base`. If the variable supports alternative configurations, examples should also be added to `.env.alt`.
- If the variable contains an external URL that is **not** an asset URL (i.e., not an image or JSON file), the appropriate CSP policy in `nextjs/csp/policies/` is updated. Policies should only be added when the relevant config option is actually enabled.
- If the variable links to an external resource (image or JSON config), the `ASSETS_ENVS` array in `deploy/scripts/download_assets.sh` is extended with the variable name.
- If the variable stores a JSON config URL, an example file is added to `deploy/tools/envs-validator/test/assets/configs/` and the `envsWithJsonConfig` array in `deploy/tools/envs-validator/index.ts` is extended.
- The PR description clearly states that a new ENV variable was added and includes example values.

## 4. Component Architecture & Reuse

- When links to **application pages** are constructed, verify that `nextjs-routes` or `nextjs/routes` utilities are used instead of string concatenation or template literals. The full list of application routes is available in `nextjs/nextjs-routes.d.ts`.

## 5. Testing with Playwright

- Flag any tests that rely on CSS class selectors (`.className`). These will break when the implementation changes. Advise the author to use roles, test IDs, text content, or other semantic selectors instead.
- Flag unnecessary use of the `testFn` pattern (i.e., `const testFn: TestFixture<...> = async (...)`). This pattern makes refactoring harder and can bypass Playwright lint rules. It should only be used when genuinely sharing test logic across multiple test suites.
- Check for unexplained magic values in test data. If a test uses a specific number or string, its meaning should be clear. Advise the author to use named constants (e.g., `const BLOCK_HEIGHT = 3947;` instead of a bare `3947`).
- Check for hardcoded URLs or values that already exist in mock files. Advise the author to import and reference them from the existing mocks to keep tests DRY and consistent.

## 6. Utilities & Best Practices

- Check whether any utility logic could be replaced by an `es-toolkit` function (e.g., `clamp`, `get`). Advise the author to prefer existing utilities over manual implementations.
- Flag unsafe type coercions such as `as unknown as MyType`. If coercion is necessary, the author should add runtime validation or a comment explaining why the cast is safe.
- Check that date and time formatting uses the shared project utilities (`Time` or `TimeWithTooltip` components). The project maintains a single consistent date format.
- Flag any commented-out code blocks. Advise the author to either implement the feature or remove the dead code entirely. TODOs are acceptable only when there is a clear follow-up plan.
