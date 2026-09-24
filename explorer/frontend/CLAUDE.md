# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Development
yarn dev                    # Start dev server
yarn build                  # Production build (Next.js only)
yarn build:next             # Full build with asset downloading and sprite building

# Linting
yarn lint:eslint            # ESLint check
yarn lint:eslint:fix        # ESLint auto-fix
yarn lint:tsc               # TypeScript type check
yarn lint:cspell            # Spell check
yarn lint:envs-validator:test  # Validate environment variable schemas

# Testing
yarn test:vitest            # Unit tests (Vitest, matches **/*.spec.ts(x))
yarn test:pw                # Playwright component/E2E tests (matches *.pw.tsx)
yarn test:pw:docker         # Playwright tests in Docker

# Assets
yarn svg:build-sprite       # Rebuild SVG sprite
yarn chakra:typegen         # Regenerate Chakra UI types
```

**Requirements:** Node >=22.14.0, npm >=10.9.0

## Architecture

**Stack:** Next.js 15 (Pages Router, not App Router), React 19, Chakra UI v3, React Query 5, Wagmi 2 / Viem 2, Valibot for schema validation, Vitest + Playwright for testing.

**Key directories:**
- `pages/` — Next.js page components (require default exports)
- `ui/` — React UI components organized by feature (~65 subdirectories)
- `lib/` — Business logic, API utilities, custom hooks, context providers
- `toolkit/` — Design system layer: `toolkit/chakra/` (custom Chakra components), `toolkit/theme/` (semantic color tokens), `toolkit/hooks/`
- `configs/app/` — Runtime app configuration (features, API endpoints, UI settings)
- `nextjs/` — Next.js config utilities: headers, rewrites, redirects, type-safe routes via `nextjs-routes`
- `mocks/` — Mock data for tests
- `deploy/tools/envs-validator/` — Environment variable validation schema and tests

**Data flow:** Pages use React Query for server state. Global UI state lives in React Context providers (`AppContextProvider`, `SettingsContextProvider`, etc.) initialized in `pages/_app.tsx`. WebSocket real-time data flows through `SocketProvider`.

**Routing:** Use `nextjs-routes` / `nextjs/routes` utilities for constructing links to application pages — never string concatenation. The full route list is in `nextjs/nextjs-routes.d.ts`.

## Design System Rules

These are enforced by ESLint and must be followed:

- **Always import from `toolkit/chakra/**`** before falling back to native Chakra UI. If a custom version exists in `toolkit/chakra/`, use it.
- **Never use hardcoded colors** (RGB, hex). Use semantic color tokens from `toolkit/theme/foundations/semanticTokens.ts` and `toolkit/theme/foundations/colors.ts` (e.g., `text.secondary`, `border.divider`, `icon.secondary`).
- **No custom `box-shadow`** — use design system shadow tokens.
- Don't override spacing on internal parts of compound components (e.g., don't add custom padding to `DialogHeader` inside a `Dialog`).
- Use `toolkit/chakra/link` instead of `next/link`.
- Use `lib/date/dayjs.ts` instead of importing `dayjs` directly.
- Date/time rendering must use the shared `Time` or `TimeWithTooltip` components.

## Global Type Declarations

- **Never use `(window as any)`** to access third-party globals. Instead, declare the property in `global.d.ts` inside the existing `declare global { interface Window { ... } }` block.
- Use `decs.d.ts` only for untyped third-party module declarations (`declare module 'foo'`).

## TypeScript Conventions

- Prefer `interface` over `type`. Use `interface extends` over `&` intersection (performance).
- No `enum` — use `as const` objects instead.
- Use top-level `import type { Foo }` not inline `import { type Foo }`.
- Default exports only when required by the framework (Next.js pages). All other exports are named.
- Declare return types on top-level module functions.
- `readonly` properties by default; omit only when genuinely mutable.
- Use `satisfies` for type validation instead of `as MyType[]` assertions.
- Outside generic functions, use `any` extremely sparingly; prefer `unknown` with proper narrowing.
- Extract magic numbers as `UPPER_SNAKE_CASE` constants above the component definition.
- Define empty array/object defaults as static constants outside components (not inline `?? []`).
- Wrap `.filter()`, `.map()`, `.reduce()` results in `useMemo` when passed as props or used as hook deps.
- Type parameters in generics are prefixed with `T` (e.g., `TKey`, `TValue`).

## Adding Environment Variables

When adding, renaming, or removing an environment variable, all of the following must be updated:

1. `docs/ENVS.md` — document name, type, whether required, default, and example
2. `configs/app/` — add to the appropriate section (`features/`, `ui.ts`, `api.ts`, etc.)
3. `deploy/tools/envs-validator/schema.ts` — add/update validation schema
4. `deploy/tools/envs-validator/test/.env.base` — add to test presets
5. `nextjs/csp/policies/` — update CSP if the variable references an external (non-asset) URL
6. `deploy/scripts/download_assets.sh` — add to `ASSETS_ENVS` if it's an asset URL
7. If it's a JSON config URL: add example to `deploy/tools/envs-validator/test/assets/configs/` and extend `envsWithJsonConfig` in `deploy/tools/envs-validator/index.ts`

## Testing

**Vitest** (unit): Files named `*.spec.ts` / `*.spec.tsx`. Run a single file with `yarn test:vitest path/to/file.spec.ts`.

**Playwright** (component/E2E): Files named `*.pw.tsx`. Three test projects run against each test: `default` (desktop Chrome 1200×750), `mobile` (iPhone 13 Pro), `dark-color-mode`. Tag tests with `@mobile` or `@dark-mode` to target specific projects; use `-@default` to exclude desktop.

- Use roles, test IDs, and text content as selectors — never CSS class selectors.
- Import mock values from existing files in `mocks/` rather than hardcoding them.
- Avoid the `testFn: TestFixture<...>` pattern unless sharing logic across multiple suites.
