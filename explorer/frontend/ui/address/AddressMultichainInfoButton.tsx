import React from 'react';

import type { Address } from 'types/api/address';

import config from 'configs/app';
import type { LinkProps } from 'toolkit/chakra/link';
import { Link } from 'toolkit/chakra/link';

const feature = config.features.multichainButton;

interface Props extends LinkProps {
  addressData?: Address;
}

const AddressMultichainInfoButton = ({ addressData, ...rest }: Props) => {
  if (!feature.isEnabled) {
    return null;
  }

  const promotedProvider = feature.providers.find((provider) => provider.promo);

  if (!promotedProvider) {
    return null;
  }

  if (!addressData || (addressData.is_contract && addressData.proxy_type !== 'eip7702')) {
    return null;
  }

  const url = (() => {
    try {
      const url = new URL(promotedProvider.urlTemplate.replace('{address}', addressData.hash));
      url.searchParams.append('utm_source', 'blockscout');
      url.searchParams.append('utm_medium', 'address');
      return url.toString();
    } catch (error) {}
    return null;
  })();

  if (!url) {
    return null;
  }

  return (
    <Link
      href={ url }
      variant="underlaid"
      external={ promotedProvider.dappId ? false : true }
      flexShrink={ 0 }
      { ...rest }
    >
      Multichain info
    </Link>
  );
};

export default React.memo(AddressMultichainInfoButton);
