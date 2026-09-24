import React from 'react';

import type { ArbitrumL2MessagesItem } from 'types/api/arbitrumL2';

import type { StatusTagType } from './StatusTag';
import StatusTag from './StatusTag';

export interface Props {
  status: ArbitrumL2MessagesItem['status'];
  isLoading?: boolean;
}

const ArbitrumL2MessageStatus = ({ status, isLoading }: Props) => {
  let type: StatusTagType;
  let text: string;

  switch (status) {
    case 'relayed': {
      type = 'ok';
      text = 'Relayed';
      break;
    }
    case 'confirmed': {
      type = 'pending';
      text = 'Ready for relay';
      break;
    }
    case 'sent': {
      type = 'pending';
      text = 'Waiting';
      break;
    }
    case 'initiated': {
      type = 'pending';
      text = 'Pending';
      break;
    }
    default:
      type = 'pending';
      text = status;
      break;
  }

  return <StatusTag type={ type } text={ text } loading={ isLoading }/>;
};

export default ArbitrumL2MessageStatus;
