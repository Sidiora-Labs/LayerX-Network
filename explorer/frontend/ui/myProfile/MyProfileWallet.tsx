import { Box, Text } from '@chakra-ui/react';
import type { UseQueryResult } from '@tanstack/react-query';
import React from 'react';

import type { UserInfo } from 'types/api/account';

import config from 'configs/app';
import { Button } from 'toolkit/chakra/button';
import { Heading } from 'toolkit/chakra/heading';
import { Link } from 'toolkit/chakra/link';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';

interface Props {
  profileQuery: UseQueryResult<UserInfo, unknown>;
  onAddWallet: () => void;
}

const MyProfileWallet = ({ profileQuery, onAddWallet }: Props) => {

  return (
    <section>
      <Heading level="2" mb={ 3 }>My linked wallet</Heading>
      <Text mb={ 3 } >
        This wallet address is used for login{ ' ' }
        { config.features.rewards.isEnabled && (
          <>
            and participation in the Merits Program.
            <Link external href="https://docs.blockscout.com/using-blockscout/merits" ml={ 1 }>
              Learn more
            </Link>
          </>
        ) }
      </Text>
      { profileQuery.data?.address_hash ? (
        <Box px={ 3 } py="18px" bgColor={{ _light: 'blackAlpha.50', _dark: 'whiteAlpha.50' }} borderRadius="base">
          <AddressEntity
            address={{ hash: profileQuery.data.address_hash }}
            fontWeight="500"
            noAltHash
          />
        </Box>
      ) : <Button size="sm" onClick={ onAddWallet }>Link wallet</Button> }
    </section>
  );
};

export default React.memo(MyProfileWallet);
