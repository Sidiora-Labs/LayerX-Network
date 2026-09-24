import { Text } from '@chakra-ui/react';
import React from 'react';

import config from 'configs/app';
import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { TruncatedTextTooltip } from 'toolkit/components/truncation/TruncatedTextTooltip';
import CopyToClipboard from 'ui/shared/CopyToClipboard';

const feature = config.features.beaconChain;

const BeaconChainValidatorLink = ({ pubkey, isLoading }: { pubkey: string; isLoading?: boolean }) => {
  if (!feature.isEnabled) {
    return null;
  }

  let content;

  if (!feature.validatorUrlTemplate) {
    content = (
      <Text
        display="inline-block"
        textOverflow="ellipsis"
        overflow="hidden"
        whiteSpace="nowrap"
        maxW="100%"
      >
        { pubkey }
      </Text>
    );
  } else {
    content = (
      <Link
        href={ feature.validatorUrlTemplate.replace('{pk}', pubkey) }
        external
        loading={ isLoading }
        overflow="hidden"
        display="grid"
        gridTemplateColumns="auto 20px"
      >
        <TruncatedTextTooltip label={ pubkey }>
          <Text overflow="hidden" textOverflow="ellipsis" whiteSpace="nowrap">{ pubkey }</Text>
        </TruncatedTextTooltip>
      </Link>
    );
  }
  return (
    <Skeleton
      display="grid"
      overflow="hidden"
      gridTemplateColumns="auto 24px"
      alignItems="center"
      loading={ isLoading }
    >
      { content }
      <CopyToClipboard
        text={ pubkey }
        type="text"
        isLoading={ isLoading }
        ml={ 1 }
      />
    </Skeleton>
  );
};

export default React.memo(BeaconChainValidatorLink);
