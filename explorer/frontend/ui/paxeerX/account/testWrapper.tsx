import type { RenderOptions } from '@testing-library/react';
import { cleanup, render as baseRender } from '@testing-library/react';
import React from 'react';

import { Provider as ChakraProvider } from 'toolkit/chakra/provider';
import { afterEach } from 'vitest';
import { wrapper as TestApp } from 'vitest/lib';

// jsdom implements no CSS media queries, and both the Chakra provider and the breakpoint hooks
// call window.matchMedia during mount, so the environment gets the API before anything renders.
if (typeof window !== 'undefined' && typeof window.matchMedia !== 'function') {
  window.matchMedia = (query: string): MediaQueryList => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: () => {},
    removeListener: () => {},
    addEventListener: () => {},
    removeEventListener: () => {},
    dispatchEvent: () => false,
  });
}

// The shared vitest wrapper carries the app contexts but no Chakra provider, and every component
// here is a Chakra component, so the specs render inside the real provider from the toolkit.
export const Wrapper = ({ children }: { children: React.ReactNode }) => (
  <ChakraProvider>
    <TestApp>{ children }</TestApp>
  </ChakraProvider>
);

// The suite runs without vitest globals, so the testing library cannot register its own teardown
// and every rendered tree would otherwise pile up in the document for the next test to find.
afterEach(cleanup);

export const render = (ui: React.ReactElement, options?: Omit<RenderOptions, 'wrapper'>) =>
  baseRender(ui, { wrapper: Wrapper, ...options });
