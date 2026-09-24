import React from 'react';

import type { MatchInt } from './utils';

interface Params {
  argType: string;
  argTypeMatchInt: MatchInt | null;
}

export default function useFormatFieldValue({ argType, argTypeMatchInt }: Params) {

  return React.useCallback((value: string | undefined) => {
    if (!value) {
      return;
    }

    if (argTypeMatchInt) {
      // we have to store all numbers as strings to avoid precision loss
      // and we cannot store them as BigInt because the NumberFormat component will not work properly
      // so we just remove all white spaces here otherwise the `viem` library will throw an error on attempt to write value to a contract
      const formattedString = value.replace(/\s/g, '');
      return formattedString;
    }

    if (argType === 'bool') {
      const formattedValue = value.toLowerCase();

      switch (formattedValue) {
        case 'true': {
          return true;
        }

        case 'false':{
          return false;
        }

        default:
          return value;
      }
    }

    return value;
  }, [ argType, argTypeMatchInt ]);
}
