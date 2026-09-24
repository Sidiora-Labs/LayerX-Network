import React from 'react';

import type { ChainInfo } from 'types/api/interop';

import TxEntity from 'ui/shared/entities/tx/TxEntity';
import type { EntityProps } from 'ui/shared/entities/tx/TxEntity';
import TxEntityInterop from 'ui/shared/entities/tx/TxEntityInterop';

type Props = {
  relay_transaction_hash?: string | null;
  relay_chain?: ChainInfo | null;
  truncation?: EntityProps['truncation'];
  isLoading?: boolean;
};

const InteropMessageDestinationTx = (props: Props) => {
  if (props.relay_chain !== undefined) {
    return (
      <TxEntityInterop
        chain={ props.relay_chain }
        hash={ props.relay_transaction_hash }
        isLoading={ props.isLoading }
        truncation={ props.truncation || 'constant' }
        noCopy
      />
    );
  }

  if (!props.relay_transaction_hash) {
    return 'N/A';
  }

  return (
    <TxEntity
      hash={ props.relay_transaction_hash }
      isLoading={ props.isLoading }
      noIcon
      truncation={ props.truncation || 'constant' }
      noCopy
    />
  );
};

export default InteropMessageDestinationTx;
