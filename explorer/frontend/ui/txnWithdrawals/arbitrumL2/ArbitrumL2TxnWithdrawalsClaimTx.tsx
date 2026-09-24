import { HStack, Spinner } from '@chakra-ui/react';
import React from 'react';
import { useWaitForTransactionReceipt } from 'wagmi';

import config from 'configs/app';
import TxEntityL1 from 'ui/shared/entities/tx/TxEntityL1';

const rollupFeature = config.features.rollup;

interface Props {
  isPending: boolean;
  hash: string;
  onSuccess: () => void;
  onError: (error: Error) => void;
}

const ArbitrumL2TxnWithdrawalsClaimTx = ({ isPending, hash, onSuccess, onError }: Props) => {
  const { status, error } = useWaitForTransactionReceipt({
    hash: hash as `0x${ string }`,
    chainId: rollupFeature.isEnabled ? Number(rollupFeature.parentChain.id) : undefined,
    query: { enabled: isPending },
  });

  React.useEffect(() => {
    switch (status) {
      case 'success':
        onSuccess();
        break;
      case 'error':
        onError(error);
        break;
    }
  }, [ status, error, onSuccess, onError ]);

  return (
    <HStack columnGap={ 2 } lineHeight="32px">
      { isPending && <Spinner size="sm"/> }
      <TxEntityL1 hash={ hash } noIcon maxW="160px" noCopy/>
    </HStack >
  );
};

export default React.memo(ArbitrumL2TxnWithdrawalsClaimTx);
