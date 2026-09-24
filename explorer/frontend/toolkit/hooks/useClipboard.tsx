import { useCopyToClipboard } from '@uidotdev/usehooks';
import React from 'react';

import useIsMobile from 'lib/hooks/useIsMobile';

import { SECOND } from '../utils/consts';
import { useDisclosure } from './useDisclosure';

// NOTE: If you don't need the disclosure and the timeout features, please use the useCopyToClipboard hook directly
export function useClipboard(text: string, timeout = SECOND) {
  const flagTimeoutRef = React.useRef<number | null>(null);
  const disclosureTimeoutRef = React.useRef<number | null>(null);
  const [ hasCopied, setHasCopied ] = React.useState(false);

  const isMobile = useIsMobile();
  const [ , copyToClipboard ] = useCopyToClipboard();
  const { open, onOpenChange } = useDisclosure();

  const copy = React.useCallback(() => {
    copyToClipboard(text);
    setHasCopied(true);
    // there is no hover on mobile, so we need to open the disclosure manually after click
    isMobile && onOpenChange({ open: true });

    disclosureTimeoutRef.current = window.setTimeout(() => {
      onOpenChange({ open: false });
    }, timeout);

    // We need to wait for the disclosure to close before setting the flag to false
    flagTimeoutRef.current = window.setTimeout(() => {
      setHasCopied(false);
    }, timeout + 200);
  }, [ text, copyToClipboard, timeout, onOpenChange, isMobile ]);

  React.useEffect(() => {
    return () => {
      if (disclosureTimeoutRef.current) {
        window.clearTimeout(disclosureTimeoutRef.current);
      }
      if (flagTimeoutRef.current) {
        window.clearTimeout(flagTimeoutRef.current);
      }
    };
  }, []);

  return React.useMemo(() => {
    return {
      hasCopied,
      copy,
      disclosure: {
        open,
        onOpenChange,
      },
    };
  }, [ hasCopied, copy, open, onOpenChange ]);
}
