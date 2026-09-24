import React from 'react';

import type { TxsSocketNoticePlace } from './types';

import * as SocketNewItemsNotice from 'ui/shared/SocketNewItemsNotice';

import useTxsSocketTypeAddress from './useTxsSocketTypeAddress';

interface Props {
  place: TxsSocketNoticePlace;
  isLoading?: boolean;
}

const TxsSocketNoticeTypeAddress = ({ place, isLoading }: Props) => {
  const { num, showErrorAlert } = useTxsSocketTypeAddress({ isLoading });

  if (num === undefined) {
    return null;
  }

  if (place === 'table') {
    return (
      <SocketNewItemsNotice.Desktop
        showErrorAlert={ showErrorAlert }
        num={ num }
        isLoading={ isLoading }
      />
    );
  }

  if (place === 'list') {
    return (
      <SocketNewItemsNotice.Mobile
        num={ num }
        showErrorAlert={ showErrorAlert }
        isLoading={ isLoading }
      />
    );
  }
};

export default React.memo(TxsSocketNoticeTypeAddress);
