import { throttle } from 'es-toolkit';
import React from 'react';

export function useIsSticky(ref: React.RefObject<HTMLElement | null>, offset = 0, isEnabled = true): boolean {
  const [ isSticky, setIsSticky ] = React.useState(false);

  const handleScroll = React.useCallback(() => {
    if (
      Number(ref.current?.getBoundingClientRect().y) < offset
    ) {
      setIsSticky(true);
    } else {
      setIsSticky(false);
    }
  }, [ ref, offset ]);

  React.useEffect(() => {
    if (!isEnabled) {
      return;
    }

    const throttledHandleScroll = throttle(handleScroll, 300);

    window.addEventListener('scroll', throttledHandleScroll);

    return () => {
      window.removeEventListener('scroll', throttledHandleScroll);
    };
  // replicate componentDidMount
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ isEnabled ]);

  return isSticky;
}
