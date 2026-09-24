import React from 'react';

// run effect only if value is updated since initial mount
const useUpdateValueEffect = (effect: () => void, value: string) => {
  const mountedRef = React.useRef(false);
  const valueRef = React.useRef<string>(undefined);
  const isChangedRef = React.useRef(false);

  React.useEffect(() => {
    mountedRef.current = true;
    valueRef.current = value;

    return () => {
      mountedRef.current = false;
      valueRef.current = undefined;
      isChangedRef.current = false;
    };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  React.useEffect(() => {
    if (mountedRef.current && (value !== valueRef.current || isChangedRef.current)) {
      isChangedRef.current = true;
      return effect();
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ value ]);
};

export default useUpdateValueEffect;
