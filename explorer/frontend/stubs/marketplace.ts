/* eslint-disable max-len */
import type { MarketplaceApp } from 'types/client/marketplace';

export const MARKETPLACE_APP: MarketplaceApp = {
  author: 'StubApp Inc.',
  id: 'stub-app',
  title: 'My cool app name',
  logo: '',
  categories: [
    'Bridge',
  ],
  shortDescription: 'Hop is a scalable rollup-to-rollup general token bridge. It allows users to send tokens from one rollup or sidechain to another almost immediately without having to wait for the networks challenge period.',
  site: 'https://example.com',
  description: 'Hop is a scalable rollup-to-rollup general token bridge. It allows users to send tokens from one rollup or sidechain to another almost immediately without having to wait for the networks challenge period.',
  external: true,
  url: 'https://example.com',
};

export const CATEGORIES: Array<string> = Array(9).fill('Bridge').map((c, i) => c + i);
