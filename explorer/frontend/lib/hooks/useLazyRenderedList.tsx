import { clamp } from 'es-toolkit';
import React from 'react';
import { useInView } from 'react-intersection-observer';

const STEP = 10;
const MIN_ITEMS_NUM = 50;

export default function useLazyRenderedList(list: Array<unknown>, isEnabled: boolean, minItemsNum: number = MIN_ITEMS_NUM) {
  const [ renderedItemsNum, setRenderedItemsNum ] = React.useState(minItemsNum);
  const { ref, inView } = useInView({
    rootMargin: '200px',
    triggerOnce: false,
    skip: !isEnabled || list.length <= minItemsNum,
  });

  React.useEffect(() => {
    if (inView) {
      setRenderedItemsNum((prev) => clamp(prev + STEP, 0, list.length));
    }
  }, [ inView, list.length ]);

  return { cutRef: ref, renderedItemsNum };
}
