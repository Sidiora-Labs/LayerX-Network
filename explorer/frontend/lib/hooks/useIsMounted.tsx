import React from 'react';

export default function useIsMounted() {
  const [ isMounted, setIsMounted ] = React.useState(false);

  React.useEffect(() => {
    setIsMounted(true);
  }, [ ]);

  return isMounted;
}
