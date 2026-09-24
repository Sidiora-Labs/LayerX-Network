import React from 'react';

import { Alert } from 'toolkit/chakra/alert';

export interface Props {
  data: unknown;
  onSettle: () => void;
  estimatedGas?: bigint;
}

const ContractMethodResultPublicClient = ({ data, estimatedGas, onSettle }: Props) => {

  React.useEffect(() => {
    onSettle();
  }, [ onSettle ]);

  if (data instanceof Error) {
    return (
      <Alert status="error" textStyle="sm" wordBreak="break-word" whiteSpace="pre-wrap">
        { 'shortMessage' in data && typeof data.shortMessage === 'string' ? data.shortMessage : data.message }
      </Alert>
    );
  }

  if (estimatedGas) {
    return (
      <Alert status="info" textStyle="sm" wordBreak="break-word" whiteSpace="pre-wrap">
        Success. Estimated gas: { estimatedGas.toLocaleString() }
      </Alert>
    );
  }

  return null;
};

export default React.memo(ContractMethodResultPublicClient);
