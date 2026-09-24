import React from 'react';

import { useFirstMountState } from './useFirstMountState';

// React effect hook that ignores the first invocation (e.g. on mount). The signature is exactly the same as the useEffect hook.
export const useUpdateEffect: typeof React.useEffect = (effect, deps) => {
  const isFirstMount = useFirstMountState();

  React.useEffect(() => {
    if (!isFirstMount) {
      return effect();
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
};
