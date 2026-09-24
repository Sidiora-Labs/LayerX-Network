import { verifiedFetch } from '@helia/verified-fetch';
import React from 'react';

export default function useLoadImageViaIpfs() {
  return React.useCallback(async(url: string) => {
    const response = await verifiedFetch(url);

    if (response.status !== 200) {
      throw new Error('Failed to load image');
    }

    const blob = await response.blob();
    const src = URL.createObjectURL(blob);
    return src;
  }, [ ]);
}
