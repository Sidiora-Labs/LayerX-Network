import { Box, chakra } from '@chakra-ui/react';
import Script from 'next/script';
import React, { useCallback, useEffect, useRef } from 'react';

import config from 'configs/app';
import { Skeleton } from 'toolkit/chakra/skeleton';

const adTextFeature = config.features.adsText;

const AD_LOAD_TIMEOUT = 1_000;

// Sevio native ad template (configured on the Sevio platform):
// <img src="[%thumbnail%]" width="20" height="20" style="display: inline-block; vertical-align: text-bottom; margin-right: 4px;" alt="" />
// <strong>Sponsored:</strong> [%sponsored%] &#8211; [%title%] <a href="[%clickURL%]" target="_blank">[%ctatext%]</a>

type Status = 'loading' | 'loaded' | 'empty';

const SevioTextAd = ({ className }: { className?: string }) => {
  const [ status, setStatus ] = React.useState<Status>('loading');
  const adRef = useRef<HTMLDivElement>(null);
  const timeoutRef = useRef<ReturnType<typeof setTimeout>>(undefined);

  useEffect(() => {
    if (!adTextFeature.isEnabled) {
      return;
    }
    const { zone, adType, inventoryId, accountId } = adTextFeature.sevio;
    window.sevioads = window.sevioads || [];
    window.sevioads.push([ { zone, adType, inventoryId, accountId } ]);
  }, []);

  // Watch for Sevio injecting content into the ad div.
  // When it does, cancel the timeout and mark the ad as loaded.
  useEffect(() => {
    const node = adRef.current;
    if (!node) {
      return;
    }

    const observer = new MutationObserver(() => {
      if (node.childNodes.length > 0) {
        clearTimeout(timeoutRef.current);
        setStatus('loaded');
        observer.disconnect();
      }
    });

    observer.observe(node, { childList: true });

    return () => {
      observer.disconnect();
    };
  }, []);

  useEffect(() => {
    return () => {
      clearTimeout(timeoutRef.current);
    };
  }, []);

  // Once the Sevio script is loaded, give it a short window to inject the ad.
  // If nothing appears within that window, assume no ad was served and hide the skeleton.
  const handleScriptLoad = useCallback(() => {
    timeoutRef.current = setTimeout(() => {
      setStatus((prev) => prev === 'loading' ? 'empty' : prev);
    }, AD_LOAD_TIMEOUT);
  }, []);

  // Script failed to load — no ad will ever appear, skip straight to empty.
  const handleScriptError = useCallback(() => {
    setStatus('empty');
  }, []);

  if (!adTextFeature.isEnabled || status === 'empty') {
    return null;
  }

  const { zone } = adTextFeature.sevio;

  return (
    <>
      { status === 'loading' && (
        <Skeleton
          loading
          className={ className }
          h={{ base: 12, lg: 6 }}
          w="100%"
          flexGrow={ 1 }
          maxW="800px"
          display="block"
        />
      ) }
      <Box
        className={ status === 'loaded' ? className : undefined }
        display={ status === 'loaded' ? undefined : 'none' }
        textStyle={{ base: 'xs', lg: 'md' }}
        color={{ base: 'text.secondary', lg: 'text.primary' }}
        css={{
          '& .sevioads *': {
            fontFamily: 'inherit',
            fontSize: 'inherit',
          },
          '& .sevioads strong': {
            color: 'inherit',
          },
          '& .sevioads a': {
            color: 'link.primary',
            _hover: {
              color: 'link.primary.hover',
            },
          },
        }}
      >
        <Script
          strategy="lazyOnload"
          src="https://cdn.adx.ws/scripts/loader.js"
          onLoad={ handleScriptLoad }
          onError={ handleScriptError }
        />
        <div ref={ adRef } className="sevioads" data-zone={ zone }/>
      </Box>
    </>
  );
};

export default chakra(SevioTextAd);
