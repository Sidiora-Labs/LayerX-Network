import { capitalize } from 'es-toolkit';
import React from 'react';

import type { DepositsItem } from 'types/api/deposits';

import StatusTag from '../statusTag/StatusTag';

const BeaconChainDepositStatusTag = ({ status, isLoading }: { status: DepositsItem['status']; isLoading: boolean }) => {
  const statusValue = (() => {
    switch (status) {
      case 'pending':
        return 'pending';
      case 'completed':
        return 'ok';
      case 'invalid':
        return 'error';
      default:
        return 'pending';
    }
  })();

  return <StatusTag type={ statusValue } text={ capitalize(status) } loading={ isLoading }/>;
};

export default BeaconChainDepositStatusTag;
