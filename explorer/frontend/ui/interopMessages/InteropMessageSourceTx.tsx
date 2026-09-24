import React from 'react';

import type { ChainInfo } from 'types/api/interop';

import type { EntityProps } from 'ui/shared/entities/tx/TxEntity';
import TxEntity from 'ui/shared/entities/tx/TxEntity';
import TxEntityInterop from 'ui/shared/entities/tx/TxEntityInterop';

type Props = {
  init_transaction_hash?: string | null;
  init_chain?: ChainInfo | null;
  isLoading?: boolean;
  truncation?: EntityProps['truncation'];
};
const InteropMessageSourceTx = (props: Props) => {
  if (props.init_chain !== undefined) {
    return (
      <TxEntityInterop
        chain={ props.init_chain }
        hash={ props.init_transaction_hash }
        isLoading={ props.isLoading }
        truncation={ props.truncation || 'constant' }
        noCopy
      />
    );
  }

  if (!props.init_transaction_hash) {
    return 'N/A';
  }

  return (
    <TxEntity
      hash={ props.init_transaction_hash }
      isLoading={ props.isLoading }
      noIcon
      truncation={ props.truncation || 'constant' }
      noCopy
    />
  );
};

export default InteropMessageSourceTx;
