import { Flex, Text } from '@chakra-ui/react';
import React from 'react';

import type { PaxeerXIdentities } from 'types/api/paxeerX';

import { route } from 'nextjs/routes';

import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';
import CopyToClipboard from 'ui/shared/CopyToClipboard';

import { listIdentities } from './utils';

export interface Props {
  identities: PaxeerXIdentities;
  isLoading?: boolean;
}

const IdentityList = ({ identities, isLoading }: Props) => {
  const entries = listIdentities(identities);

  if (entries.length === 0) {
    return <Text color="text.secondary">This account has no Paxeer X identities yet.</Text>;
  }

  return (
    <Flex flexDirection="column" rowGap={ 3 } data-label="paxeer-x-identities">
      { entries.map((entry) => (
        <Flex
          key={ entry.kind }
          columnGap={ 2 }
          alignItems="center"
          flexWrap="wrap"
          data-identity={ entry.kind }
        >
          <Skeleton loading={ isLoading } minW="140px" color="text.secondary">
            { entry.label }
          </Skeleton>
          <Skeleton loading={ isLoading } overflow="hidden" textOverflow="ellipsis" maxW="100%">
            { entry.kind === 'evm' ? (
              <Link href={ route({ pathname: '/address/[hash]', query: { hash: entry.value } }) }>
                { entry.value }
              </Link>
            ) : (
              <Text as="span" wordBreak="break-all">{ entry.value }</Text>
            ) }
          </Skeleton>
          <CopyToClipboard text={ entry.value } isLoading={ isLoading }/>
        </Flex>
      )) }
    </Flex>
  );
};

export default React.memo(IdentityList);
