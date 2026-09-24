import type { GrowthBook } from '@growthbook/growthbook-react';
import React from 'react';

import { SECOND } from 'toolkit/utils/consts';

export default function useLoadFeatures(growthBook: GrowthBook | undefined) {
  React.useEffect(() => {
    if (!growthBook) {
      return;
    }

    growthBook.setAttributes({
      ...growthBook.getAttributes(),
      timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
      language: window.navigator.language,
    });

    growthBook.loadFeatures({ timeout: SECOND });
  }, [ growthBook ]);
}
