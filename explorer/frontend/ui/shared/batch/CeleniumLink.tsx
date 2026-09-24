import { Flex, Icon } from '@chakra-ui/react';
import React from 'react';

import config from 'configs/app';
// eslint-disable-next-line no-restricted-imports
import celeniumIcon from 'icons/brands/celenium.svg';
import hexToBase64 from 'lib/hexToBase64';
import { Link } from 'toolkit/chakra/link';

const feature = config.features.rollup;

interface Props {
  commitment: string;
  namespace: string;
  height: number;
  fallback?: React.ReactNode;
}

function getCeleniumUrl(props: Props) {
  try {
    if (!feature.isEnabled || !feature.DA.celestia.celeniumUrl) {
      return undefined;
    }

    const url = new URL(feature.DA.celestia.celeniumUrl);

    url.searchParams.set('commitment', hexToBase64(props.commitment));
    url.searchParams.set('hash', hexToBase64(props.namespace));
    url.searchParams.set('height', String(props.height));

    return url.toString();
  } catch (error) {}
}

const CeleniumLink = (props: Props) => {
  const url = getCeleniumUrl(props);

  if (!url) {
    return props.fallback ?? null;
  }

  return (
    <Flex alignItems="center" columnGap={ 2 }>
      <Icon as={ celeniumIcon } boxSize={ 5 }/>
      <Link external href={ getCeleniumUrl(props) }>Blob page</Link>
    </Flex>
  );
};

export default React.memo(CeleniumLink);
