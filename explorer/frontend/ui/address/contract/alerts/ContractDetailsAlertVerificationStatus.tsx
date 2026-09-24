import { Box } from '@chakra-ui/react';
import React from 'react';

import type { Address } from 'types/api/address';
import type { SmartContract } from 'types/api/contract';

import { Alert } from 'toolkit/chakra/alert';
import { Link } from 'toolkit/chakra/link';

import ContractDetailsVerificationButton from '../ContractDetailsVerificationButton';

interface Props {
  data: SmartContract | undefined;
  isLoading: boolean;
  addressData: Address;
}

const ContractDetailsAlertVerificationStatus = ({ data, isLoading, addressData }: Props) => {
  if (!data || !data.is_verified) {
    return null;
  }

  const sourceElement = (() => {
    if (data?.is_verified_via_eth_bytecode_db) {
      return (
        <>
          <span>This contract has been { data.is_partially_verified ? 'partially ' : '' }verified using </span>
          <Link
            href="https://docs.blockscout.com/devs/verification/ethereum-bytecode-database-microservice"
            external
          >
            Blockscout Bytecode Database
          </Link>
        </>
      );
    }

    if (data?.is_verified_via_sourcify) {
      return (
        <>
          <span>This contract has been { data.is_partially_verified ? 'partially ' : '' }verified via Sourcify. </span>
          { data.sourcify_repo_url && <Link href={ data.sourcify_repo_url } textStyle="md" external>View contract in Sourcify repository</Link> }
        </>
      );
    }

    return null;
  })();

  if (!data.is_partially_verified) {
    return (
      <Alert status="success" loading={ isLoading } descriptionProps={{ whiteSpace: 'pre-wrap' }}>
        <span>Contract source code verified (exact match){ sourceElement ? '. ' : '' }</span>
        { sourceElement }
      </Alert>
    );
  }

  return (
    <Alert status="success" loading={ isLoading } descriptionProps={{ alignItems: 'center', flexWrap: 'wrap', rowGap: 2, columnGap: 3 }}>
      <span>Contract source code verified ({ data.is_partially_verified ? 'partial' : 'exact' } match){ sourceElement ? '.' : '' }</span>
      <ContractDetailsVerificationButton
        isLoading={ isLoading }
        addressHash={ addressData.hash }
      />
      { sourceElement && <Box w="100%">{ sourceElement }</Box> }
    </Alert>
  );
};

export default React.memo(ContractDetailsAlertVerificationStatus);
