import { Box, chakra, Flex } from '@chakra-ui/react';
import React from 'react';

import type { ChainInfo } from 'types/api/interop';

import { route } from 'nextjs-routes';

import { Image } from 'toolkit/chakra/image';
import { Tooltip } from 'toolkit/chakra/tooltip';
import IconSvg from 'ui/shared/IconSvg';

import { distributeEntityProps } from '../base/utils';
import * as AddressEntity from './AddressEntity';

interface Props extends Omit<AddressEntity.EntityProps, 'chain'> {
  chain: ChainInfo | null;
}

const IconStub = () => {
  return (
    <Flex
      position="absolute"
      bottom="-2px"
      right="4px"
      alignItems="center"
      justifyContent="center"
      borderRadius="base"
      background={{ _light: 'gray.100', _dark: 'gray.700' }}
      width="14px"
      height="14px"
      border="1px solid"
      borderColor="bg.primary"
    >
      <IconSvg
        name="networks/icon-placeholder"
        width="10px"
        height="10px"
        color="icon.primary"
      />
    </Flex>
  );
};

const AddressEntityInterop = ({ chain, ...props }: Props) => {
  const partsProps = distributeEntityProps(props);

  const href = chain?.instance_url ? chain.instance_url.replace(/\/$/, '') + route({
    pathname: '/address/[hash]',
    query: {
      ...props.query,
      hash: props.address.hash,
    },
  }) : null;

  const addressIcon = (
    <Box position="relative">
      <AddressEntity.Icon { ...partsProps.icon }/>
      { !props.isLoading && (
        chain?.chain_logo ? (
          <Image
            position="absolute"
            bottom="-3px"
            right="4px"
            src={ chain.chain_logo }
            alt={ chain.chain_name || 'external chain logo' }
            fallback={ <IconStub/> }
            width="14px"
            height="14px"
            borderRadius="base"
          />
        ) : (
          <IconStub/>
        )
      ) }
    </Box>
  );

  return (
    <AddressEntity.Container className={ props.className }>
      { chain && (
        <Tooltip content={ `Address on ${ chain.chain_name ? chain.chain_name : 'external chain' } (chain id ${ chain.chain_id })` }>
          { addressIcon }
        </Tooltip>
      ) }
      { !chain && addressIcon }
      { href ? (
        <AddressEntity.Link { ...partsProps.link } href={ href } external>
          <AddressEntity.Content { ...partsProps.content }/>
        </AddressEntity.Link>
      ) : (
        <Box overflow="hidden">
          <AddressEntity.Content { ...partsProps.content }/>
        </Box>
      ) }
      <AddressEntity.Copy { ...partsProps.copy }/>
    </AddressEntity.Container>
  );
};

export default chakra(AddressEntityInterop);
