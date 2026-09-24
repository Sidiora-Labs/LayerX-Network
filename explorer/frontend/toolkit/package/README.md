# Blockscout UI Toolkit

A comprehensive collection of reusable Chakra UI components and theme system for Blockscout's projects. This toolkit provides a consistent design system and UI components to maintain visual consistency across Blockscout applications.

## Features

- 🎨 Pre-configured Chakra UI theme with Blockscout's design system
- 🧩 Reusable UI components built on Chakra UI
- 🌓 Built-in dark mode support
- 📱 Responsive and accessible components
- 🔍 TypeScript support with proper type definitions

## Installation

### Package Installation

Install the package using your preferred package manager:

```bash
# Using npm
npm install @blockscout/ui-toolkit

# Using yarn
yarn add @blockscout/ui-toolkit
```

### Required Dependencies

Ensure you have the following peer dependencies installed:

```json
{
  "dependencies": {
    "@blockscout/ui-toolkit": "latest",
    "@chakra-ui/react": ">=3.33.0",
    "@emotion/react": ">=11.14.0",
    "@uidotdev/usehooks": ">=2.4.1",
    "d3": ">=7.6.1",
    "dayjs": ">=1.11.5",
    "dom-to-image": ">=2.6.0",
    "es-toolkit": ">=1.39.10",
    "next": ">=15.2.3",
    "next-themes": ">=0.4.4",
    "react": ">=18.3.1",
    "react-dom": ">=18.3.1",
    "react-hook-form": ">=7.52.1"
  },
  "devDependencies": {
    "@chakra-ui/cli": ">=3.33.0",
    "@types/node": "^20",
    "@types/react": "18.3.12",
    "@types/react-dom": "18.3.1",
    "typescript": "5.4.2"
  }
}
```

## Quick Start

### 1. Theme Setup

Create a `theme.ts` file in your project and configure the Blockscout theme:

```tsx
// Basic setup
import { theme } from '@blockscout/ui-toolkit';
export default theme;
```

Or extend the theme with custom overrides:

```tsx
import { createSystem } from '@chakra-ui/react';
import { themeConfig } from '@blockscout/ui-toolkit';

const customOverrides = {
  // Add your custom theme overrides here
  theme: {
    semanticTokens: {
      colors: {
        brand: {
          primary: { value: '#5353D3' }
        },
      },
    },
  },
};

export default createSystem(themeConfig, customOverrides);
```

### 2. Provider Setup

Wrap your application with the ChakraProvider:

```tsx
import { ChakraProvider } from '@chakra-ui/react';
import { Button } from '@blockscout/ui-toolkit';
import theme from './theme';

function App() {
  return (
    <ChakraProvider theme={theme}>
      <Button>Click me</Button>
    </ChakraProvider>
  );
}
```

### 3. TypeScript Support

Add the following script to your `package.json` to generate Chakra UI type definitions:

```json
{
  "scripts": {
    "chakra:typegen": "chakra typegen ./src/theme.ts"
  }
}
```

## Development

### Local Development

1. Clone the repository and install dependencies:
```bash
yarn
```

2. Start the development server:
```bash
yarn dev
```

3. Build the package:
```bash
yarn build
```

### Publishing

#### Manual Publishing

1. Update the package version:
```bash
npm version <version-tag>
```

2. Build the package:
```bash
npm run build
```

3. Publish to NPM:
```bash
npm publish --access public
```

#### Automated Publishing

Use the `toolkit-npm-publisher.yml` GitHub Actions workflow for automated publishing.

## Contributing

We welcome contributions! Please follow these steps:

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Submit a pull request

## Support

For issues, feature requests, or questions, please open an issue in the repository.

## License

This project is licensed under the GNU General Public License v3.