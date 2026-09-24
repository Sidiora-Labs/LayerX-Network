import React from 'react';
import { scroller } from 'react-scroll';

import type { SmartContractMethod } from './types';

export const getElementId = (data: SmartContractMethod) => {
  if ('method_id' in data) {
    return data.method_id;
  }

  if ('name' in data) {
    return data.name;
  }

  return data.type;
};

export const getElementName = (data: SmartContractMethod) => {
  return `method_${ getElementId(data) }`;
};

export default function useScrollToMethod(data: Array<SmartContractMethod>, onScroll: (indices: Array<string>) => void) {
  React.useEffect(() => {
    const hash = window.location.hash.replace('#', '');

    if (!hash) {
      return;
    }

    const index = data.findIndex((item) => getElementId(item) === hash);
    if (index > -1) {
      scroller.scrollTo(getElementName(data[ index ]), {
        duration: 500,
        smooth: true,
        offset: -100,
      });
      onScroll([ String(index) ]);
    }
  }, [ data, onScroll ]);
}
