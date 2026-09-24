import React from 'react';

import type { TxAction } from 'types/api/txAction';

import config from 'configs/app';
import TxDetailsActionsInterpretation from 'ui/tx/details/txDetailsActions/TxDetailsActionsInterpretation';
import TxDetailsActionsRaw from 'ui/tx/details/txDetailsActions/TxDetailsActionsRaw';

type Props = {
  isTxDataLoading: boolean;
  actions?: Array<TxAction>;
  hash?: string;
};

const TxDetailsActions = ({ isTxDataLoading, actions, hash }: Props) => {
  if (config.features.txInterpretation.isEnabled) {
    return <TxDetailsActionsInterpretation hash={ hash } isTxDataLoading={ isTxDataLoading }/>;
  }

  /* if tx interpretation is not configured, show tx actions from tx info */
  if (actions && actions.length > 0) {
    return <TxDetailsActionsRaw actions={ actions } isLoading={ isTxDataLoading }/>;
  }

  return null;
};

export default TxDetailsActions;
