import { includeIgnoreFile } from '@eslint/compat';
import jsPlugin from '@eslint/js';
import nextJsPlugin from '@next/eslint-plugin-next';
import stylisticPlugin from '@stylistic/eslint-plugin';
import reactQueryPlugin from '@tanstack/eslint-plugin-query';
import consistentDefaultExportNamePlugin from 'eslint-plugin-consistent-default-export-name';
import importPlugin from 'eslint-plugin-import';
import importHelpersPlugin from 'eslint-plugin-import-helpers';
import jsxA11yPlugin from 'eslint-plugin-jsx-a11y';
import playwrightPlugin from 'eslint-plugin-playwright';
import reactPlugin from 'eslint-plugin-react';
import reactHooksPlugin from 'eslint-plugin-react-hooks';
import * as regexpPlugin from 'eslint-plugin-regexp';
import vitestPlugin from 'eslint-plugin-vitest';
import globals from 'globals';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import tseslint from 'typescript-eslint';

const RESTRICTED_MODULES = {
  paths: [
    { name: 'dayjs', message: 'Please use lib/date/dayjs.ts instead of directly importing dayjs' },
    { name: '@chakra-ui/icons', message: 'Using @chakra-ui/icons is prohibited. Please use regular svg-icon instead (see examples in "icons/" folder)' },
    { name: '@metamask/providers', message: 'Please lazy-load @metamask/providers or use useProvider hook instead' },
    { name: '@metamask/post-message-stream', message: 'Please lazy-load @metamask/post-message-stream or use useProvider hook instead' },
    { name: 'playwright/TestApp', message: 'Please use render() fixture from test() function of playwright/lib module' },
    {
      name: '@chakra-ui/react',
      importNames: [
        'Menu', 'useToast', 'useDisclosure', 'useClipboard', 'Tooltip', 'Skeleton', 'IconButton', 'Button', 'ButtonGroup', 'Link', 'LinkBox', 'LinkOverlay',
        'Dialog', 'DialogRoot', 'DialogContent', 'DialogHeader', 'DialogCloseTrigger', 'DialogBody',
        'Tag', 'Switch', 'Image', 'Popover', 'PopoverTrigger', 'PopoverContent', 'PopoverBody', 'PopoverFooter',
        'DrawerRoot', 'DrawerBody', 'DrawerContent', 'DrawerOverlay', 'DrawerBackdrop', 'DrawerTrigger', 'Drawer',
        'Alert', 'AlertIcon', 'AlertTitle', 'AlertDescription',
        'Select', 'SelectRoot', 'SelectControl', 'SelectContent', 'SelectItem', 'SelectValueText',
        'Heading', 'Badge', 'Tabs', 'Show', 'Hide', 'Checkbox', 'CheckboxGroup',
        'Table', 'TableRoot', 'TableBody', 'TableHeader', 'TableRow', 'TableCell',
        'Menu', 'MenuRoot', 'MenuTrigger', 'MenuContent', 'MenuItem', 'MenuTriggerItem', 'MenuRadioItemGroup', 'MenuContextTrigger',
        'Rating', 'RatingGroup', 'Textarea', 'Progress', 'ProgressCircle',
        'EmptyState',
      ],
      message: 'Please use corresponding component or hook from "toolkit" instead',
    },
    {
      name: 'next/link',
      importNames: [ 'default' ],
      message: 'Please use toolkit/chakra/link component instead',
    },
  ],
  patterns: [
    'icons/*',
  ],
};

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const gitignorePath = path.resolve(__dirname, '.gitignore');

/** @type {import('eslint').Linter.Config[]} */
export default tseslint.config(
  includeIgnoreFile(gitignorePath),

  { files: [ '**/*.{js,mjs,cjs,ts,jsx,tsx}', '**/*.pw.tsx' ] },

  { ignores: [
    'deploy/tools/',
    'public/',
    '.git/',
    'next.config.js',
  ] },

  { languageOptions: { globals: { ...globals.browser, ...globals.node } } },

  { settings: { react: { version: 'detect' } } },

  jsPlugin.configs.recommended,

  {
    plugins: {
      '@typescript-eslint': tseslint.plugin,
    },
    languageOptions: {
      parser: tseslint.parser,
      parserOptions: {
        projectService: true,
      },
    },
    rules: {
      '@typescript-eslint/array-type': [ 'error', {
        'default': 'generic',
        readonly: 'generic',
      } ],
      '@typescript-eslint/consistent-type-imports': [ 'error' ],
      '@typescript-eslint/naming-convention': [ 'error',
        {
          selector: 'default',
          format: [ 'camelCase' ],
          leadingUnderscore: 'allow',
          trailingUnderscore: 'forbid',
        },
        {
          selector: 'import',
          leadingUnderscore: 'allow',
          format: [ 'camelCase', 'PascalCase' ],
        },
        {
          selector: 'class',
          format: [ 'PascalCase' ],
        },
        {
          selector: 'enum',
          format: [ 'PascalCase', 'UPPER_CASE' ],
        },
        {
          selector: 'enumMember',
          format: [ 'camelCase', 'PascalCase', 'UPPER_CASE' ],
        },
        {
          selector: 'function',
          format: [ 'camelCase', 'PascalCase' ],
        },
        {
          selector: 'interface',
          format: [ 'PascalCase' ],
        },
        {
          selector: 'method',
          format: [ 'camelCase', 'snake_case', 'UPPER_CASE' ],
          leadingUnderscore: 'allow',
        },
        {
          selector: 'parameter',
          format: [ 'camelCase', 'PascalCase' ],
          leadingUnderscore: 'allow',
        },
        {
          selector: 'property',
          format: null,
        },
        {
          selector: 'typeAlias',
          format: [ 'PascalCase' ],
        },
        {
          selector: 'typeLike',
          format: [ 'PascalCase' ],
        },
        {
          selector: 'typeParameter',
          format: [ 'PascalCase', 'UPPER_CASE' ],
        },
        {
          selector: 'variable',
          format: [ 'camelCase', 'PascalCase', 'UPPER_CASE' ],
          leadingUnderscore: 'allow',
        },
      ],
      '@typescript-eslint/no-empty-function': [ 'off' ],
      '@typescript-eslint/no-unused-vars': [ 'error', { caughtErrors: 'none', ignoreRestSiblings: true } ],
      '@typescript-eslint/no-use-before-define': 'off',
      '@typescript-eslint/no-useless-constructor': [ 'error' ],
      '@typescript-eslint/no-explicit-any': [ 'error', { ignoreRestArgs: true } ],
      '@typescript-eslint/no-unused-expressions': [ 'error', {
        allowShortCircuit: true,
        allowTernary: true,
      } ],
    },
  },
  {
    // disable type-aware linting on JS files
    files: [ '**/*.{js,mjs}' ],
    ...tseslint.configs.disableTypeChecked,
  },

  {
    plugins: {
      react: reactPlugin,
    },
    rules: {
      'react/jsx-key': 'error',
      'react/jsx-no-bind': [ 'error', {
        ignoreRefs: true,
      } ],
      'react/jsx-curly-brace-presence': [ 'error', {
        props: 'never',
        children: 'never',
      } ],
      'react/jsx-curly-spacing': [ 'error', {
        when: 'always',
        children: true,
        spacing: {
          objectLiterals: 'never',
        },
      } ],
      'react/jsx-equals-spacing': [ 'error', 'never' ],
      'react/jsx-fragments': [ 'error', 'syntax' ],
      'react/jsx-no-duplicate-props': 'error',
      'react/jsx-no-target-blank': 'off',
      'react/jsx-no-useless-fragment': 'error',
      'react/jsx-tag-spacing': [ 'error', {
        afterOpening: 'never',
        beforeSelfClosing: 'never',
        closingSlash: 'never',
      } ],
      'react/jsx-wrap-multilines': [ 'error', {
        declaration: 'parens-new-line',
        assignment: 'parens-new-line',
        'return': 'parens-new-line',
        arrow: 'parens-new-line',
        condition: 'parens-new-line',
        logical: 'parens-new-line',
        prop: 'parens-new-line',
      } ],
      'react/no-access-state-in-setstate': 'error',
      'react/no-deprecated': 'error',
      'react/no-direct-mutation-state': 'error',
      'react/no-find-dom-node': 'off',
      'react/no-redundant-should-component-update': 'error',
      'react/no-render-return-value': 'error',
      'react/no-string-refs': 'off',
      'react/no-unknown-property': 'error',
      'react/no-unused-state': 'error',
      'react/require-optimization': [ 'error' ],
      'react/void-dom-elements-no-children': 'error',
    },
  },

  {
    plugins: {
      '@next/next': nextJsPlugin,
    },
    rules: {
      ...nextJsPlugin.configs.recommended.rules,
      ...nextJsPlugin.configs['core-web-vitals'].rules,
    },
  },

  {
    plugins: { '@tanstack/query': reactQueryPlugin },
  },

  {
    ...playwrightPlugin.configs['flat/recommended'],
    files: [ '**/*.pw.tsx' ],
    rules: {
      ...playwrightPlugin.configs['flat/recommended'].rules,
      'playwright/no-standalone-expect': 'off', // this rules does not work correctly with extended test functions
    },
  },

  {
    plugins: { 'react-hooks': reactHooksPlugin },
    ignores: [ '**/*.pw.tsx', 'playwright/**' ],
    rules: {
      'react-hooks/rules-of-hooks': 'error',
      'react-hooks/exhaustive-deps': 'error',
    },
  },

  {
    files: [ '**/*.spec.{ts,js,jsx,tsx}' ],
    plugins: { vitest: vitestPlugin },
    rules: {
      ...vitestPlugin.configs.recommended.rules,
    },
    settings: {
      vitest: {
        typecheck: true,
      },
    },
    languageOptions: {
      globals: {
        ...vitestPlugin.environments.env.globals,
        fetchMock: true,
      },
    },
  },

  regexpPlugin.configs['flat/recommended'],

  {
    plugins: {
      'import': importPlugin,
    },
    rules: {
      'import/no-duplicates': 'error',
    },
  },

  {
    plugins: {
      'import-helpers': importHelpersPlugin,
    },
    rules: {
      'import-helpers/order-imports': [
        'error',
        {
          newlinesBetween: 'always',
          groups: [
            'module',
            '/types/',
            [ '/^nextjs/' ],
            [
              '/^configs/',
              '/^data/',
              '/^deploy/',
              '/^icons/',
              '/^lib/',
              '/^mocks/',
              '/^pages/',
              '/^playwright/',
              '/^stubs/',
              '/^theme/',
              '/^toolkit/',
              '/^ui/',
              '/^vitest/',
            ],
            [ 'parent', 'sibling', 'index' ],
          ],
          alphabetize: { order: 'asc', ignoreCase: true },
        },
      ],
    },
  },

  {
    plugins: {
      'jsx-a11y': jsxA11yPlugin,
    },
    languageOptions: { parserOptions: { ecmaFeatures: { jsx: true } } },
  },

  {
    plugins: {
      'consistent-default-export-name': consistentDefaultExportNamePlugin,
    },
    files: [
      'ui/**/[A-Z]*.tsx',
    ],
    ignores: [
      '**/*.pw.*',
      '**/*.pwstory.*',
      '**/pages/**',
    ],
    rules: {
      'consistent-default-export-name/default-export-match-filename': [ 'error', null ],
    },
  },

  {
    plugins: {
      '@stylistic': stylisticPlugin,
    },
    rules: {
      // replacement for @typescript-eslint
      '@stylistic/indent': [ 'error', 2 ],
      '@stylistic/brace-style': [ 'error', '1tbs' ],
      '@stylistic/member-delimiter-style': [ 'error' ],
      '@stylistic/type-annotation-spacing': 'error',

      // replacement for eslint
      '@stylistic/array-bracket-spacing': [ 'error', 'always' ],
      '@stylistic/arrow-spacing': [ 'error', { before: true, after: true } ],
      '@stylistic/comma-dangle': [ 'error', 'always-multiline' ],
      '@stylistic/comma-spacing': [ 'error' ],
      '@stylistic/comma-style': [ 'error', 'last' ],
      '@stylistic/curly-newline': [ 'error', { multiline: true, minElements: 1 } ],
      '@stylistic/eol-last': 'error',
      '@stylistic/jsx-quotes': [ 'error', 'prefer-double' ],
      '@stylistic/key-spacing': [ 'error', {
        beforeColon: false,
        afterColon: true,
      } ],
      '@stylistic/keyword-spacing': 'error',
      '@stylistic/linebreak-style': [ 'error', 'unix' ],
      '@stylistic/lines-around-comment': [ 'error', {
        beforeBlockComment: true,
        allowBlockStart: true,
      } ],
      '@stylistic/no-mixed-operators': [ 'error', {
        groups: [
          [ '&&', '||' ],
        ],
      } ],
      '@stylistic/no-mixed-spaces-and-tabs': 'error',
      '@stylistic/no-multiple-empty-lines': [ 'error', {
        max: 1,
        maxEOF: 0,
        maxBOF: 0,
      } ],
      '@stylistic/no-multi-spaces': 'error',
      '@stylistic/no-trailing-spaces': 'error',
      '@stylistic/object-curly-spacing': [ 'error', 'always' ],
      '@stylistic/operator-linebreak': [ 'error', 'after' ],
      '@stylistic/quote-props': [ 'error', 'as-needed', {
        keywords: true,
        numbers: true,
      } ],
      '@stylistic/quotes': [ 'error', 'single', {
        allowTemplateLiterals: 'always',
      } ],
      '@stylistic/semi': [ 'error', 'always' ],
      '@stylistic/space-before-function-paren': [ 'error', { anonymous: 'never', named: 'never', asyncArrow: 'never', 'catch': 'always' } ],
      '@stylistic/space-before-blocks': [ 'error', 'always' ],
      '@stylistic/space-in-parens': [ 'error', 'never' ],
      '@stylistic/space-infix-ops': 'error',
      '@stylistic/space-unary-ops': 'off',
      '@stylistic/template-curly-spacing': [ 'error', 'always' ],
      '@stylistic/wrap-iife': [ 'error', 'inside' ],
    },
  },

  {
    rules: {
      // disabled in favor of @typescript-eslint and @stylistic
      'no-use-before-define': 'off',
      'no-useless-constructor': 'off',
      'no-unused-vars': 'off',
      'no-empty': [ 'error', { allowEmptyCatch: true } ],
      'no-unused-expressions': 'off',

      // this is checked by typescript compiler
      'no-redeclare': 'off',

      // rules customizations
      eqeqeq: [ 'error', 'allow-null' ],
      'id-match': [ 'error', '^[\\w$]+$' ],
      'max-len': [ 'error', 160, 4 ],
      'no-console': 'error',
      'no-implicit-coercion': [ 'error', {
        number: true,
        'boolean': true,
        string: true,
      } ],
      'no-nested-ternary': 'error',
      'no-multi-str': 'error',
      'no-spaced-func': 'error',
      'no-with': 'error',
      'object-shorthand': 'off',
      'one-var': [ 'error', 'never' ],
      'prefer-const': 'error',

      // restricted imports and properties
      'no-restricted-imports': [ 'error', RESTRICTED_MODULES ],
      'no-restricted-properties': [ 2, {
        object: 'process',
        property: 'env',
        // FIXME: restrict the rule only NEXT_PUBLIC variables
        message: 'Please use configs/app/index.ts to import any NEXT_PUBLIC environment variables. For other properties please disable this rule for a while.',
      } ],
    },
  },
  {
    files: [
      'pages/**',
      'nextjs/**',
      'playwright/**',
      'deploy/scripts/**',
      'deploy/tools/**',
      'middleware.ts',
      'instrumentation*.ts',
      '*.config.ts',
      '*.config.js',
    ],
    rules: {
      // for configs allow to consume env variables from process.env directly
      'no-restricted-properties': 'off',
    },
  },
  {
    files: [
      'toolkit/chakra/**',
      'toolkit/components/**',
      'toolkit/package/**',
    ],
    rules: {
      // for toolkit components allow to import @chakra-ui/react directly
      'no-restricted-imports': 'off',
    },
  },
);
