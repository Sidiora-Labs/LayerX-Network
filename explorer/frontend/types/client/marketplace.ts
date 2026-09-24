import type { Chain } from 'viem';

import type { ExternalChain } from 'types/externalChains';

import type config from 'configs/app';

export type MarketplaceAppBase = {
  id: string;
  author: string;
  site?: string;
  external?: boolean;
  title: string;
  description: string;
  logo: string;
  logoDarkMode?: string;
  shortDescription: string;
  categories: Array<string>;
  url: string;
  internalWallet?: boolean;
  priority?: number;
};

export type MarketplaceAppRating = {
  rating?: number;
  ratingsTotalCount?: number;
  userRating?: number;
};

export type MarketplaceAppSocialInfo = {
  twitter?: string;
  telegram?: string;
  github?: string | Array<string>;
  discord?: string;
};

export type MarketplaceApp = MarketplaceAppBase & MarketplaceAppSocialInfo & MarketplaceAppRating;

export enum MarketplaceCategory {
  ALL = 'All',
  FAVORITES = 'Favorites',
}

export interface EssentialDappsChainConfig extends ExternalChain {
  app_config?: Pick<typeof config, 'app' | 'chain'> & {
    apis: Pick<typeof config['apis'], 'general'>;
  };
  contracts?: Chain['contracts'];
}

export type EssentialDappsConfig = {
  swap?: {
    url: string;
    chains: Array<string>;
    fee: string;
    integrator: string;
  };
  revoke?: {
    chains: Array<string>;
  };
  multisend?: {
    chains: Array<string>;
    posthogKey?: string;
    posthogHost?: string;
  };
};

export interface MarketplaceTitles {
  entity_name: string;
  menu_item: string;
  title: string;
  subtitle_essential_dapps: string;
  subtitle_list: string;
}
