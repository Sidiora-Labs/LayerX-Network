import { Flex } from '@chakra-ui/react';
import { useRouter } from 'next/router';
import React from 'react';

import { useMultichainContext } from 'lib/contexts/multichain';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import ChainIcon from 'ui/shared/externalChains/ChainIcon';
import PageTitle from 'ui/shared/Page/PageTitle';
import Sol2UmlDiagram from 'ui/sol2uml/Sol2UmlDiagram';

const Sol2Uml = () => {
  const router = useRouter();
  const multichainContext = useMultichainContext();

  const addressHash = router.query.address?.toString() || '';

  const chainInfo = multichainContext?.chain ? (
    <Flex display="inline-flex" alignItems="center">
      <span>on</span>
      <ChainIcon data={ multichainContext.chain } ml={ 1 } mr={ 2 }/>
      <span>{ multichainContext.chain.app_config.chain.name }</span>
    </Flex>
  ) : null;

  return (
    <>
      <PageTitle title="Solidity UML diagram"/>
      <Flex mb={ 10 } flexWrap="wrap" columnGap={ 1 }>
        <span>For contract</span>
        <AddressEntity
          address={{ hash: addressHash, is_contract: true }}
          noCopy
        />
        { chainInfo }
      </Flex>
      <Sol2UmlDiagram addressHash={ addressHash }/>
    </>
  );
};

export default Sol2Uml;
