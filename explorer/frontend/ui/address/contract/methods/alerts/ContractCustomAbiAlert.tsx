import React from 'react';

import { Alert } from 'toolkit/chakra/alert';

interface Props {
  isLoading?: boolean;
}

const ContractCustomAbiAlert = ({ isLoading }: Props) => {
  return (
    <Alert status="warning" loading={ isLoading }>
      Note: Contract with custom ABI is only meant for debugging purpose and it is the user’s responsibility to ensure that the provided ABI
      matches the contract, otherwise errors may occur or results returned may be incorrect.
      Blockscout is not responsible for any losses that arise from the use of Read & Write contract.
    </Alert>
  );
};

export default React.memo(ContractCustomAbiAlert);
