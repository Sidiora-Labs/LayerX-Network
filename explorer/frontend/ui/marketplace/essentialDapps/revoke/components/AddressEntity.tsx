import { chakra } from '@chakra-ui/react';
import { mainnet } from 'viem/chains';
import { useEnsName } from 'wagmi';

import type { EntityProps } from 'ui/shared/entities/address/AddressEntity';
import AddressEntityBase from 'ui/shared/entities/address/AddressEntity';

function AddressEntity({ address, ...props }: EntityProps) {
  const ensQuery = useEnsName({
    address: address.hash as `0x${ string }`,
    chainId: mainnet.id,
  });

  return <AddressEntityBase address={{ ...address, name: ensQuery.data }} { ...props }/>;
}

export default chakra(AddressEntity);
