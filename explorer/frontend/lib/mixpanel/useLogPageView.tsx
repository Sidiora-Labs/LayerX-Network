import { usePathname } from 'next/navigation';
import { useRouter } from 'next/router';
import React from 'react';

import type { ColorThemeId } from 'types/settings';

import config from 'configs/app';
import * as cookies from 'lib/cookies';
import getQueryParamString from 'lib/router/getQueryParamString';
import { getDefaultColorTheme } from 'lib/settings/colorTheme';
import { useColorMode } from 'toolkit/chakra/color-mode';

import getPageType from './getPageType';
import getTabName from './getTabName';
import logEvent from './logEvent';
import { EventTypes } from './utils';

export default function useLogPageView(isInitialized: boolean) {
  const router = useRouter();
  const pathname = usePathname();

  const tab = getQueryParamString(router.query.tab);
  const page = getQueryParamString(router.query.page);
  const id = getQueryParamString(router.query.id);
  const { colorMode } = useColorMode();

  React.useEffect(() => {
    if (!config.features.mixpanel.isEnabled || !isInitialized) {
      return;
    }

    const cookieColorTheme = cookies.get(cookies.NAMES.COLOR_THEME) as ColorThemeId | undefined;

    logEvent(EventTypes.PAGE_VIEW, {
      'Page type': getPageType(router.pathname),
      Tab: getTabName(tab),
      Page: page || undefined,
      Source: router.pathname === '/essential-dapps/[id]' ? id : undefined,
      'Color mode': colorMode,
      'Color theme': cookieColorTheme || getDefaultColorTheme(colorMode),
    });
    // these are only deps that should trigger the effect
    // in some scenarios page type is not changing (e.g navigation from one address page to another),
    // but we still want to log page view
    // so we use pathname from 'next/navigation' instead of router.pathname from 'next/router' as deps
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ isInitialized, page, pathname, tab, colorMode ]);
}
