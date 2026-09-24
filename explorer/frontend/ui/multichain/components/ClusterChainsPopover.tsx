import { Box, chakra, VStack } from '@chakra-ui/react';
import React from 'react';

import type * as multichain from '@blockscout/multichain-aggregator-types';

import { route } from 'nextjs/routes';

import multichainConfig from 'configs/multichain';
import { Button } from 'toolkit/chakra/button';
import { Link } from 'toolkit/chakra/link';
import { PopoverBody, PopoverContent, PopoverRoot, PopoverTrigger } from 'toolkit/chakra/popover';
import ChainIcon from 'ui/shared/externalChains/ChainIcon';
import IconSvg from 'ui/shared/IconSvg';

interface Props {
  addressHash: string;
  data: multichain.GetAddressResponse | undefined;
  isLoading: boolean;
}

const ClusterChainsPopover = ({ addressHash, data, isLoading }: Props) => {

  if (!data) {
    return null;
  }

  const chains = multichainConfig()?.chains;
  const activeChainsIds = Object.keys(data.chain_infos ?? {});
  const activeChains = chains?.filter((chain) => activeChainsIds.includes(String(chain.id))) ?? [];

  if (!isLoading && activeChains.length === 0) {
    return null;
  }

  return (
    <PopoverRoot>
      <Box>
        <PopoverTrigger>
          <Button
            size="sm"
            variant="dropdown"
            aria-label="Chains this address has interacted with"
            px={ 2 }
            fontWeight={ 500 }
            flexShrink={ 0 }
            columnGap={ 1 }
            loadingSkeleton={ isLoading }
          >
            <IconSvg name="pie_chart" boxSize={ 5 }/>
            { activeChains.length } Chain{ activeChains.length > 1 ? 's' : '' }
          </Button>
        </PopoverTrigger>
      </Box>
      <PopoverContent w="auto" maxH="400px" overflowY="auto">
        <PopoverBody >
          <chakra.span color="text.secondary" textStyle="xs">Chains this address has interacted with</chakra.span>
          <VStack gap={ 2 } mt={ 1 } alignItems="flex-start">
            { activeChains.map((chain) => (
              <Link
                key={ chain.id }
                href={ route({
                  pathname: '/address/[hash]',
                  query: {
                    hash: addressHash,
                    utm_source: 'multichain-explorer',
                    utm_medium: 'address',
                  },
                }, { chain, external: true }) }
                external
                display="flex"
                alignItems="center"
                py="7px"
              >
                <ChainIcon data={ chain } mr={ 2 }/>
                { chain.name }
              </Link>
            )) }
          </VStack>
        </PopoverBody>
      </PopoverContent>
    </PopoverRoot>
  );
};

export default React.memo(ClusterChainsPopover);
