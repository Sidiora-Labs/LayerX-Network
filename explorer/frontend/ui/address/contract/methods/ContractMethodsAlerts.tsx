import { VStack } from '@chakra-ui/react';
import React from 'react';

import type { SmartContractConflictingImplementation, SmartContractProxyType } from 'types/api/contract';

import ContractCodeProxyPattern from '../alerts/ContractDetailsAlertProxyPattern';
import ConnectWalletAlert from './alerts/ConnectWalletAlert';
import ContractCustomAbiAlert from './alerts/ContractCustomAbiAlert';

interface Props {
  isLoading?: boolean;
  isCustomAbi?: boolean;
  proxyType?: SmartContractProxyType;
  conflictingImplementations?: Array<SmartContractConflictingImplementation>;
}

const ContractMethodsAlerts = ({ isLoading, isCustomAbi, proxyType, conflictingImplementations }: Props) => {
  return (
    <VStack gap={ 1 } alignItems="stretch">
      <ConnectWalletAlert isLoading={ isLoading }/>
      { proxyType && <ContractCodeProxyPattern isLoading={ isLoading } type={ proxyType } conflictingImplementations={ conflictingImplementations }/> }
      { isCustomAbi && <ContractCustomAbiAlert isLoading={ isLoading }/> }
    </VStack>
  );
};

export default React.memo(ContractMethodsAlerts);
