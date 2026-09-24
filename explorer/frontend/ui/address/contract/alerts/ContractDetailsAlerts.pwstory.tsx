import React from 'react';

import useSocketChannel from 'lib/socket/useSocketChannel';

import type { Props } from './ContractDetailsAlerts';
import ContractDetailsAlerts from './ContractDetailsAlerts';

const ContractDetailsAlertsPwStory = (props: Props) => {
  const channel = useSocketChannel({
    topic: `addresses:${ props.addressData.hash.toLowerCase() }`,
    isDisabled: false,
  });

  return <ContractDetailsAlerts { ...props } channel={ channel }/>;
};

export default ContractDetailsAlertsPwStory;
