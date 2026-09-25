import type { ThemingConfig } from '@chakra-ui/react';

import type { ExcludeUndefined } from 'types/utils';

const shadows: ExcludeUndefined<ThemingConfig['tokens']>['shadows'] = {
  action_bar: { value: '0 4px 4px -4px rgb(0 0 0 / 10%), 0 2px 4px -4px rgb(0 0 0 / 6%)' },
  // The product elevation pair: cards rest, overlays lift.
  card: { value: '0 1px 2px rgb(0 0 0 / 0.04), 0 4px 16px rgb(0 0 0 / 0.05)' },
  overlay: { value: '0 8px 40px rgb(0 0 0 / 0.14)' },
  size: {
    xs: { value: '0px 0px 0px 1px rgba(0, 0, 0, 0.05)' },
    sm: { value: '0px 1px 2px 0px rgba(0, 0, 0, 0.05)' },
    base: { value: '0px 1px 2px 0px rgba(0, 0, 0, 0.06), 0px 1px 3px 0px rgba(0, 0, 0, 0.1)' },
    md: { value: '0px 2px 4px -1px rgba(0, 0, 0, 0.06), 0px 4px 6px -1px rgba(0, 0, 0, 0.1)' },
    lg: { value: '0px 4px 6px -2px rgba(0, 0, 0, 0.05), 0px 10px 15px -3px rgba(0, 0, 0, 0.1)' },
    xl: { value: '0px 10px 10px -5px rgba(0, 0, 0, 0.04), 0px 20px 25px -5px rgba(0, 0, 0, 0.1)' },
    '2xl': { value: '0px 15px 50px -12px rgba(0, 0, 0, 0.25)' },
  },
  'dark-lg': { value: '0px 15px 40px 0px rgba(0, 0, 0, 0.4), 0px 5px 10px 0px rgba(0, 0, 0, 0.2), 0px 0px 0px 1px rgba(0, 0, 0, 0.1)' },
};

export default shadows;
