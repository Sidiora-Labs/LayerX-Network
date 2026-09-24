import { Box, Flex } from '@chakra-ui/react';
import React from 'react';

import { Tooltip } from 'toolkit/chakra/tooltip';
import * as EntityBase from 'ui/shared/entities/base/components';

import type { ContentProps } from './AddressEntity';
import AddressEntity from './AddressEntity';

const AddressEntityContentProxy = (props: ContentProps) => {
  const implementations = props.address.implementations;

  if (!implementations || implementations.length === 0) {
    return null;
  }

  const colNum = Math.min(implementations.length, 3);
  const nameTag = props.address.metadata?.tags.find(tag => tag.tagType === 'name')?.name;

  const implementationName = implementations.length === 1 && implementations[0].name ? implementations[0].name : undefined;

  const content = (
    <>
      <Box fontWeight={ 600 }>
        Proxy contract
        { props.address.name ? ` (${ props.address.name })` : '' }
      </Box>
      <AddressEntity
        address={{ hash: props.address.hash, filecoin: props.address.filecoin }}
        noLink
        noIcon
        noHighlight
        noTooltip
        justifyContent="center"
        w="100%"
      />
      <Box fontWeight={ 600 } mt={ 2 }>
        Implementation{ implementations.length > 1 ? 's' : '' }
        { implementationName ? ` (${ implementationName })` : '' }
      </Box>
      <Flex flexWrap="wrap" columnGap={ 3 }>
        { implementations.map((item) => (
          <AddressEntity
            key={ item.address_hash }
            address={{ hash: item.address_hash, filecoin: { robust: item.filecoin_robust_address } }}
            noLink
            noIcon
            noHighlight
            noTooltip
            minW={ `calc((100% - ${ colNum - 1 } * 12px) / ${ colNum })` }
            flex={ 1 }
            justifyContent={ colNum === 1 ? 'center' : undefined }
          />
        )) }
      </Flex>
    </>
  );

  return (
    <Tooltip content={ content } interactive contentProps={{ maxW: { base: 'calc(100vw - 8px)', lg: '410px' } }} triggerProps={{ minW: 0 }}>
      <Box display="inline-flex" w="100%">
        <EntityBase.Content
          { ...props }
          truncation={ nameTag || implementationName || props.address.name ? 'tail' : props.truncation }
          text={ nameTag || implementationName || props.address.name || props.altHash || props.address.hash }
          noTooltip
        />
      </Box>
    </Tooltip>
  );
};

export default React.memo(AddressEntityContentProxy);
