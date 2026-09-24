import { debounce } from 'es-toolkit';
import { useEffect, useState } from 'react';

export function useViewportSize(debounceTime = 100) {
  const [ viewportSize, setViewportSize ] = useState({ width: 0, height: 0 });

  useEffect(() => {
    setViewportSize({ width: window.innerWidth, height: window.innerHeight });

    const resizeHandler = debounce(() => {
      setViewportSize({ width: window.innerWidth, height: window.innerHeight });
    }, debounceTime);
    const resizeObserver = new ResizeObserver(resizeHandler);

    resizeObserver.observe(document.body);
    return function cleanup() {
      resizeObserver.unobserve(document.body);
    };
  }, [ debounceTime ]);

  return viewportSize;
}
