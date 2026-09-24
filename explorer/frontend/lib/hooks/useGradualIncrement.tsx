import React from 'react';

const DURATION = 300;

export default function useGradualIncrement(initialValue: number): [number, (inc: number) => void] {
  const [ num, setNum ] = React.useState(initialValue);
  const queue = React.useRef<number>(0);
  const timeoutId = React.useRef(0);
  const delay = React.useRef(0);

  const incrementDelayed = React.useCallback(() => {
    if (queue.current === 0) {
      return;
    }

    queue.current--;
    setNum(prev => prev + 1);
    timeoutId.current = 0;
  }, []);

  const increment = React.useCallback((inc: number) => {
    if (inc < 1) {
      return;
    }

    queue.current += inc;

    if (!timeoutId.current) {
      timeoutId.current = window.setTimeout(incrementDelayed, 0);
    }
  }, [ incrementDelayed ]);

  React.useEffect(() => {
    if (queue.current > 0 && !timeoutId.current) {
      if (!delay.current) {
        delay.current = DURATION / queue.current;
      } else if (delay.current > DURATION / queue.current) {
        // in case if queue size is increased since last DOM update
        delay.current = DURATION / queue.current;
      }
      timeoutId.current = window.setTimeout(incrementDelayed, delay.current);
    }
  }, [ incrementDelayed, num ]);

  React.useEffect(() => {
    return () => {
      window.clearTimeout(timeoutId.current);
    };
  }, []);

  return [ num, increment ];
}
