import React from 'react';

export default function useIsInitialLoading(isLoading: boolean | undefined) {
  const [ isInitialLoading, setIsInitialLoading ] = React.useState(Boolean(isLoading));

  React.useEffect(() => {
    if (!isLoading) {
      setIsInitialLoading(false);
    }
  }, [ isLoading ]);

  return isInitialLoading;
}
