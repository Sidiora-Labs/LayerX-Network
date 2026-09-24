import { Grid, GridItem, HStack, Text, VStack } from '@chakra-ui/react';
import React from 'react';

import type { SmartContractConflictingImplementation } from 'types/api/contract';

import { Button } from 'toolkit/chakra/button';
import { DialogActionTrigger, DialogBody, DialogContent, DialogHeader, DialogRoot, DialogTrigger } from 'toolkit/chakra/dialog';
import { Link } from 'toolkit/chakra/link';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';

import { PROXY_TYPES } from './utils';

interface Props {
  data: Array<SmartContractConflictingImplementation>;
  children: React.ReactNode;
}

const ConflictingImplementationsModal = ({ data, children }: Props) => {
  return (
    <DialogRoot size={{ lgDown: 'full', lg: 'md' }}>
      <DialogTrigger>
        { children }
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>Detected proxy implementations</DialogHeader>
        <DialogBody>
          <Text>
            Multiple proxy patterns were detected for this contract.
            This may be due to an unsupported custom proxy design or due to a malicious proxy spoofing attempt.
            Review carefully.
          </Text>
          <VStack alignItems="stretch" mt={ 6 } textStyle="sm">
            { data.map((item) => {
              const addressNum = item.implementations.length;
              const addressText = addressNum === 1 ? 'Implementation:' : 'Implementations:';
              const proxyType = PROXY_TYPES[item.proxy_type]?.name || PROXY_TYPES.unknown?.name;

              return (
                <Grid
                  key={ item.proxy_type }
                  templateColumns="115px minmax(0px, 1fr)"
                  w="100%"
                  p={ 4 }
                  columnGap={ 5 }
                  rowGap={ 2 }
                  borderRadius="md"
                  bgColor={{ _light: 'blackAlpha.50', _dark: 'whiteAlpha.50' }}
                >
                  <GridItem>Proxy type:</GridItem>
                  <GridItem>{ proxyType }</GridItem>
                  <GridItem>{ addressText }</GridItem>
                  <GridItem>
                    <VStack alignItems="stretch">
                      { item.implementations.map((implementation) => (
                        <AddressEntity
                          key={ implementation.address_hash }
                          address={{ hash: implementation.address_hash, name: implementation.name }}
                        />
                      )) }
                    </VStack>
                  </GridItem>
                </Grid>
              );
            }) }
          </VStack>
          <HStack mt={ 6 } gap={ 6 }>
            <DialogActionTrigger asChild>
              <Button>Got it, thanks</Button>
            </DialogActionTrigger>
            <Link external noIcon href="https://discord.com/invite/blockscout">
              Contact us
            </Link>
          </HStack>
        </DialogBody>
      </DialogContent>
    </DialogRoot>
  );
};

export default React.memo(ConflictingImplementationsModal);
