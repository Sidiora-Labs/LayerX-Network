import type { ContractCodeIde } from 'types/client/contract';
import { type NavItemExternal, type NavigationLayout, type NavigationPromoBannerConfig } from 'types/client/navigation';
import { HOME_STATS_WIDGET_IDS, type ChainIndicatorId, type HeroBannerConfig, type HomeStatsWidgetId } from 'types/homepage';
import type { NetworkExplorer } from 'types/networks';
import type { ColorThemeId } from 'types/settings';
import type { FontFamily } from 'types/ui';

import { COLOR_THEMES, type ColorTheme } from 'lib/settings/colorTheme';

import * as features from './features';
import * as views from './ui/views';
import { getEnvValue, getExternalAssetFilePath, parseEnvJson } from './utils';

const homePageStats: Array<HomeStatsWidgetId> = (() => {
  const parsedValue = parseEnvJson<Array<HomeStatsWidgetId>>(getEnvValue('NEXT_PUBLIC_HOMEPAGE_STATS'));

  if (!Array.isArray(parsedValue)) {
    const rollupFeature = features.rollup;

    if (rollupFeature.isEnabled && [ 'zkEvm', 'zkSync', 'arbitrum' ].includes(rollupFeature.type)) {
      return [ 'latest_batch', 'average_block_time', 'total_txs', 'wallet_addresses', 'gas_tracker' ];
    }

    return [ 'total_blocks', 'average_block_time', 'total_txs', 'wallet_addresses', 'gas_tracker' ];
  }

  return parsedValue.filter((item) => HOME_STATS_WIDGET_IDS.includes(item));
})();

const highlightedRoutes = (() => {
  const parsedValue = parseEnvJson<Array<string>>(getEnvValue('NEXT_PUBLIC_NAVIGATION_HIGHLIGHTED_ROUTES'));
  return Array.isArray(parsedValue) ? parsedValue : [];
})();

const defaultColorTheme = (() => {
  const envValue = getEnvValue('NEXT_PUBLIC_COLOR_THEME_DEFAULT') as ColorThemeId | undefined;
  return COLOR_THEMES.find((theme) => theme.id === envValue) as ColorTheme | undefined;
})();

const navigationPromoBanner = (() => {
  const envValue = parseEnvJson<NavigationPromoBannerConfig>(getEnvValue('NEXT_PUBLIC_NAVIGATION_PROMO_BANNER_CONFIG'));
  return envValue || undefined;
})();

const UI = Object.freeze({
  navigation: {
    logo: {
      'default': getExternalAssetFilePath('NEXT_PUBLIC_NETWORK_LOGO'),
      dark: getExternalAssetFilePath('NEXT_PUBLIC_NETWORK_LOGO_DARK'),
    },
    icon: {
      'default': getExternalAssetFilePath('NEXT_PUBLIC_NETWORK_ICON'),
      dark: getExternalAssetFilePath('NEXT_PUBLIC_NETWORK_ICON_DARK'),
    },
    highlightedRoutes,
    otherLinks: parseEnvJson<Array<NavItemExternal>>(getEnvValue('NEXT_PUBLIC_OTHER_LINKS')) || [],
    layout: (getEnvValue('NEXT_PUBLIC_NAVIGATION_LAYOUT') || 'vertical') as NavigationLayout,
    promoBanner: navigationPromoBanner,
  },
  featuredNetworks: {
    items: getExternalAssetFilePath('NEXT_PUBLIC_FEATURED_NETWORKS'),
    allLink: getEnvValue('NEXT_PUBLIC_FEATURED_NETWORKS_ALL_LINK'),
    mode: (getEnvValue('NEXT_PUBLIC_FEATURED_NETWORKS_MODE') || 'list') as 'tabs' | 'list',
  },
  footer: {
    links: getExternalAssetFilePath('NEXT_PUBLIC_FOOTER_LINKS'),
    frontendVersion: getEnvValue('NEXT_PUBLIC_GIT_TAG'),
    frontendCommit: getEnvValue('NEXT_PUBLIC_GIT_COMMIT_SHA'),
  },
  homepage: {
    charts: parseEnvJson<Array<ChainIndicatorId>>(getEnvValue('NEXT_PUBLIC_HOMEPAGE_CHARTS')) || [],
    stats: homePageStats,
    heroBanner: parseEnvJson<HeroBannerConfig>(getEnvValue('NEXT_PUBLIC_HOMEPAGE_HERO_BANNER_CONFIG')),
    highlights: getExternalAssetFilePath('NEXT_PUBLIC_HOMEPAGE_HIGHLIGHTS_CONFIG'),
  },
  views,
  indexingAlert: {
    blocks: {
      isHidden: getEnvValue('NEXT_PUBLIC_HIDE_INDEXING_ALERT_BLOCKS') === 'true' ? true : false,
    },
    intTxs: {
      isHidden: getEnvValue('NEXT_PUBLIC_HIDE_INDEXING_ALERT_INT_TXS') === 'true' ? true : false,
    },
  },
  maintenanceAlert: {
    message: getEnvValue('NEXT_PUBLIC_MAINTENANCE_ALERT_MESSAGE'),
  },
  apiKeysAlert: {
    message: getEnvValue('NEXT_PUBLIC_API_KEYS_ALERT_MESSAGE'),
  },
  explorers: {
    items: parseEnvJson<Array<NetworkExplorer>>(getEnvValue('NEXT_PUBLIC_NETWORK_EXPLORERS')) || [],
  },
  ides: {
    items: parseEnvJson<Array<ContractCodeIde>>(getEnvValue('NEXT_PUBLIC_CONTRACT_CODE_IDES')) || [],
  },
  hasContractAuditReports: getEnvValue('NEXT_PUBLIC_HAS_CONTRACT_AUDIT_REPORTS') === 'true' ? true : false,
  colorTheme: {
    'default': defaultColorTheme,
    overrides: parseEnvJson<Record<string, unknown>>(getEnvValue('NEXT_PUBLIC_COLOR_THEME_OVERRIDES')) || {},
  },
  fonts: {
    heading: parseEnvJson<FontFamily>(getEnvValue('NEXT_PUBLIC_FONT_FAMILY_HEADING')),
    body: parseEnvJson<FontFamily>(getEnvValue('NEXT_PUBLIC_FONT_FAMILY_BODY')),
  },
  maxContentWidth: getEnvValue('NEXT_PUBLIC_MAX_CONTENT_WIDTH_ENABLED') === 'false' ? false : true,
  nativeCoinPrice: {
    isHidden: getEnvValue('NEXT_PUBLIC_HIDE_NATIVE_COIN_PRICE') === 'true' ? true : false,
  },
});

export default UI;
