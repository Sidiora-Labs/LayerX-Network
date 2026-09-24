import React from 'react';

import * as tac from '@blockscout/tac-operation-lifecycle-types';

import { getTacOperationStatus } from 'lib/operations/tac';
import { Tooltip } from 'toolkit/chakra/tooltip';

import StatusTag from './StatusTag';

interface Props {
  status: tac.OperationType;
  isLoading?: boolean;
  noTooltip?: boolean;
}

const TacOperationStatus = ({ status, isLoading, noTooltip }: Props) => {
  const text = getTacOperationStatus(status);

  if (!text) {
    return null;
  }

  switch (status) {
    case tac.OperationType.ERROR:
    case tac.OperationType.INSUFFICIENT_FEE:
      return <StatusTag type="error" text={ text } loading={ isLoading }/>;
    case tac.OperationType.ROLLBACK:
      return (
        <Tooltip
          // eslint-disable-next-line max-len
          content="The cross‑chain operation was reverted and the original assets and state were returned to the sender after a failure on the destination chain"
          disabled={ noTooltip }
        >
          <StatusTag type="error" text={ text } loading={ isLoading }/>
        </Tooltip>
      );
    case tac.OperationType.PENDING: {
      return <StatusTag type="pending" text={ text } loading={ isLoading }/>;
    }
    default: {
      return <StatusTag type="ok" text={ text } loading={ isLoading }/>;
    }
  }
};

export default React.memo(TacOperationStatus);
