import React from 'react';

import type { TxsSocketNoticePlace, TxsSocketType } from './types';

import useIsMobile from 'lib/hooks/useIsMobile';

import TxsSocketNoticeTypeAddress from './TxsSocketNoticeTypeAddress';
import TxsSocketNoticeTypeAll from './TxsSocketNoticeTypeAll';

interface Props {
  type: TxsSocketType;
  place: TxsSocketNoticePlace;
  isLoading?: boolean;
}

const TxsSocketNotice = ({ type, place, isLoading }: Props) => {
  const isMobile = useIsMobile();

  if ((isMobile && place === 'table') || (!isMobile && place === 'list')) {
    return null;
  }

  switch (type) {
    case 'txs_home':
    case 'txs_validated':
    case 'txs_pending': {
      return <TxsSocketNoticeTypeAll type={ type } place={ place } isLoading={ isLoading }/>;
    }
    case 'address_txs': {
      return <TxsSocketNoticeTypeAddress place={ place } isLoading={ isLoading }/>;
    }

    default:
      return null;
  }
};

export default React.memo(TxsSocketNotice);
