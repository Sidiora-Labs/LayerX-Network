import { usePathname } from 'next/navigation';
import { useRouter } from 'next/router';
import React from 'react';

import config from 'configs/app';
import getQueryParamString from 'lib/router/getQueryParamString';

export default function useNotifyOnNavigation() {
  const router = useRouter();
  const pathname = usePathname();
  const tab = getQueryParamString(router.query.tab);

  React.useEffect(() => {
    if (config.features.metasuites.isEnabled) {
      window.postMessage({ source: 'APP_ROUTER', type: 'PATHNAME_CHANGED' }, window.location.origin);
    }
  }, [ pathname ]);

  React.useEffect(() => {
    if (config.features.metasuites.isEnabled) {
      window.postMessage({ source: 'APP_ROUTER', type: 'TAB_CHANGED' }, window.location.origin);
    }
  }, [ tab ]);
}
