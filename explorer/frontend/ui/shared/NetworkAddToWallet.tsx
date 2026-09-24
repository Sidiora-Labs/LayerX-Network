import React from 'react';

import config from 'configs/app';
import useAddChainClick from 'lib/web3/useAddChainClick';
import useProvider from 'lib/web3/useProvider';
import { WALLETS_INFO } from 'lib/web3/wallets';
import { Button } from 'toolkit/chakra/button';
import IconSvg from 'ui/shared/IconSvg';

interface Props {
  source: 'Footer' | 'Top bar';
  onAddSuccess?: () => void;
}

const NetworkAddToWallet = ({ source, onAddSuccess }: Props) => {
  const { data: { wallet } = {} } = useProvider();

  const handleClick = useAddChainClick({ source, onSuccess: onAddSuccess });

  if (!wallet) {
    return null;
  }

  const walletInfo = WALLETS_INFO[wallet];

  return (
    <Button
      variant="outline"
      size="2xs"
      borderWidth="1px"
      fontWeight="500"
      color={ walletInfo.color }
      borderColor={ walletInfo.color }
      onClick={ handleClick }
      _hover={{
        color: 'link.primary.hover',
        borderColor: 'link.primary.hover',
      }}
    >
      <IconSvg name={ walletInfo.icon } boxSize={ 3 }/>
      Add { config.chain.name }
    </Button>
  );
};

export default React.memo(NetworkAddToWallet);
