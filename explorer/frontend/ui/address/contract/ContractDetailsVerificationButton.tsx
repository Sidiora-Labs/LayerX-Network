import React from 'react';

import { route } from 'nextjs-routes';

import config from 'configs/app';
import { useMultichainContext } from 'lib/contexts/multichain';
import { Button } from 'toolkit/chakra/button';
import type { LinkProps } from 'toolkit/chakra/link';
import { Link } from 'toolkit/chakra/link';

interface Props extends LinkProps {
  isLoading: boolean;
  addressHash: string;
}

const ContractDetailsVerificationButton = ({ isLoading, addressHash, ...rest }: Props) => {

  const multichainContext = useMultichainContext();

  const href = (() => {
    if (multichainContext) {
      const searchParams = new URLSearchParams();
      searchParams.set('contracts', `${ multichainContext.chain.id }:${ addressHash }`);
      return `https://vera.blockscout.com?${ searchParams.toString() }`;
    }
    return route({ pathname: '/address/[hash]/contract-verification', query: { hash: addressHash } });
  })();

  return (
    <Link
      href={ href }
      target={ config.features.multichain.isEnabled ? '_blank' : undefined }
      flexShrink={ 0 }
      asChild
      { ...rest }
    >
      <Button
        size="sm"
        loadingSkeleton={ isLoading }
      >
        Verify & publish
      </Button>
    </Link>
  );
};

export default React.memo(ContractDetailsVerificationButton);
