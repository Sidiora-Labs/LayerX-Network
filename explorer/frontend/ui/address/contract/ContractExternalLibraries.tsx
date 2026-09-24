import { Box, Flex, Separator, VStack } from '@chakra-ui/react';
import React from 'react';

import type { SmartContractExternalLibrary } from 'types/api/contract';

import useIsMobile from 'lib/hooks/useIsMobile';
import { Alert } from 'toolkit/chakra/alert';
import { Button } from 'toolkit/chakra/button';
import { DialogBody, DialogContent, DialogHeader, DialogRoot } from 'toolkit/chakra/dialog';
import { Heading } from 'toolkit/chakra/heading';
import { PopoverRoot, PopoverBody, PopoverContent, PopoverTrigger } from 'toolkit/chakra/popover';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { useDisclosure } from 'toolkit/hooks/useDisclosure';
import { apos } from 'toolkit/utils/htmlEntities';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import IconSvg from 'ui/shared/IconSvg';

interface Props {
  className?: string;
  data: Array<SmartContractExternalLibrary>;
  isLoading?: boolean;
}

const Item = (data: SmartContractExternalLibrary) => {
  return (
    <Flex flexDir="column" py={ 2 } w="100%" rowGap={ 1 }>
      <Box>{ data.name }</Box>
      <AddressEntity
        address={{ hash: data.address_hash, is_contract: true }}
        query={{ tab: 'contract' }}
        fontSize="sm"
        fontWeight="500"
        target="_blank"
      />
    </Flex>
  );
};

const ContractExternalLibraries = ({ className, data, isLoading }: Props) => {
  const { open, onToggle, onOpenChange } = useDisclosure();
  const isMobile = useIsMobile();

  if (isLoading) {
    return <Skeleton loading h={ 8 } w="150px" borderRadius="base"/>;
  }

  if (data.length === 0) {
    return null;
  }

  const button = (
    <Button
      className={ className }
      size="sm"
      variant="dropdown"
      onClick={ onToggle }
      expanded={ open }
      fontWeight={ 600 }
      px={ 2 }
      gap={ 0 }
      aria-label="View external libraries"
    >
      <span>{ data.length } { data.length > 1 ? 'Libraries' : 'Library' } </span>
      <IconSvg name="status/warning" boxSize={ 5 } color="orange.400" ml="2px"/>
      <IconSvg name="arrows/east-mini" transform={ open ? 'rotate(90deg)' : 'rotate(-90deg)' } transitionDuration="faster" boxSize={ 5 } ml={ 2 }/>
    </Button>
  );

  const content = (
    <>
      <Heading size="sm" level="3">External libraries ({ data.length })</Heading>
      <Alert status="warning" mt={ 4 }>
        The linked library{ apos }s source code may not be the real one.
        Check the source code at the library address (if any) if you want to be sure in case if there is any library linked
      </Alert>
      <VStack
        separator={ <Separator/> }
        gap={ 2 }
        mt={ 4 }
        maxH={{ lg: '50vh' }}
        overflowY="scroll"
      >
        { data.map((item) => <Item key={ item.address_hash } { ...item }/>) }
      </VStack>
    </>
  );

  if (isMobile) {
    return (
      <>
        { button }
        <DialogRoot open={ open } onOpenChange={ onOpenChange } size="full">
          <DialogContent paddingTop={ 4 }>
            <DialogHeader/>
            <DialogBody>
              { content }
            </DialogBody>
          </DialogContent>
        </DialogRoot>
      </>
    );
  }

  return (
    <PopoverRoot open={ open } onOpenChange={ onOpenChange }>
      <PopoverTrigger>
        { button }
      </PopoverTrigger>
      <PopoverContent w="400px">
        <PopoverBody >
          { content }
        </PopoverBody>
      </PopoverContent>
    </PopoverRoot>
  );
};

export default ContractExternalLibraries;
