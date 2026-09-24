import { Box, Flex } from '@chakra-ui/react';
import React from 'react';

import config from 'configs/app';
import { Image } from 'toolkit/chakra/image';
import { Link } from 'toolkit/chakra/link';
import { Tooltip } from 'toolkit/chakra/tooltip';
import TxEntity from 'ui/shared/entities/tx/TxEntity';

const externalTxFeature = config.features.externalTxs;

interface Props {
  data: Array<string>;
}

const TxExternalTxs: React.FC<Props> = ({ data }) => {
  if (!externalTxFeature.isEnabled) {
    return null;
  }

  const content = (
    <Box textStyle="sm">
      <Flex alignItems="center" gap={ 2 } fontSize="md" mb={ 3 }>
        <Image src={ externalTxFeature.chainLogoUrl } alt={ externalTxFeature.chainName } width={ 5 } height={ 5 }/>
        { externalTxFeature.chainName } transaction{ data.length > 1 ? 's' : '' }
      </Flex>
      <Flex flexDirection="column" gap={ 2 } w="100%" maxHeight="460px" overflowY="auto">
        { data.map((txHash) => (
          <TxEntity
            key={ txHash }
            hash={ txHash }
            href={ externalTxFeature.explorerUrlTemplate.replace('{hash}', txHash) }
            link={{ external: true }}
            noCopy
            // tooltip inside tooltip doesn't work well
            noTooltip
          />
        )) }
      </Flex>
    </Box>
  );

  return (
    <Tooltip
      content={ content }
      variant="popover"
      interactive
      positioning={{ placement: 'bottom-end' }}
      openDelay={ 300 }
      contentProps={{ w: { base: '300px', lg: '460px' } }}
    >
      <Link
        display="inline-flex"
        alignItems="center"
        gap={ 2 }
        textDecorationLine="underline"
        textDecorationStyle="dashed"
      >
        <Image src={ externalTxFeature.chainLogoUrl } alt={ externalTxFeature.chainName } boxSize={ 5 }/>
        { `${ data.length } ${ externalTxFeature.chainName } txn${ data.length > 1 ? 's' : '' }` }
      </Link>
    </Tooltip>
  );
};

export default TxExternalTxs;
