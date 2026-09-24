import { Box, Flex, Spinner, Text } from '@chakra-ui/react';
import React from 'react';

import delay from 'lib/delay';
import { Button } from 'toolkit/chakra/button';
import { IconButton } from 'toolkit/chakra/icon-button';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import IconSvg from 'ui/shared/IconSvg';

import UserWalletAutoConnectAlert from '../UserWalletAutoConnectAlert';

interface Props {
  address: string;
  domain?: string;
  isAutoConnectDisabled?: boolean;
  isReconnecting?: boolean;
  onDisconnect: () => void;
  onOpenWallet: () => void;
}

const UserWalletMenuContent = ({ isAutoConnectDisabled, address, domain, isReconnecting, onDisconnect, onOpenWallet }: Props) => {

  const handleOpenWalletClick = React.useCallback(async() => {
    await delay(100);
    onOpenWallet();
  }, [ onOpenWallet ]);

  return (
    <Box>
      { isAutoConnectDisabled && <UserWalletAutoConnectAlert/> }
      <Text fontSize="sm" fontWeight={ 600 } mb={ 1 }>My wallet</Text>
      <Text fontSize="sm" mb={ 5 } fontWeight={ 400 } color="text.secondary">
        Your wallet is used to interact with apps and contracts in the explorer.
      </Text>
      <Flex alignItems="center" columnGap={ 2 } justifyContent="space-between">
        <AddressEntity
          address={{ hash: address, ens_domain_name: domain }}
          truncation="dynamic"
          fontSize="sm"
          fontWeight={ 700 }
        />
        { isReconnecting ? <Spinner size="sm" m="2px" flexShrink={ 0 }/> : (
          <IconButton
            aria-label="Open wallet"
            variant="icon_secondary"
            size="2xs"
            onClick={ handleOpenWalletClick }
          >
            <IconSvg name="gear"/>
          </IconButton>
        ) }
      </Flex>
      <Button size="sm" width="full" variant="outline" onClick={ onDisconnect } mt={ 6 }>
        Disconnect
      </Button>
    </Box>
  );
};

export default React.memo(UserWalletMenuContent);
