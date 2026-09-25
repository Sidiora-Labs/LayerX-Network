import type { ThemingConfig } from '@chakra-ui/react';

import type { ExcludeUndefined } from 'types/utils';

// The product radius scale: 8px chips and tiles, 12px inputs and small cards,
// 16px cards, 20px modals, 24px sheets.
export const radii: ExcludeUndefined<ThemingConfig['tokens']>['radii'] = {
  none: { value: '0' },
  sm: { value: '8px' },
  base: { value: '12px' },
  md: { value: '16px' },
  lg: { value: '20px' },
  xl: { value: '24px' },
  full: { value: '9999px' },
};
