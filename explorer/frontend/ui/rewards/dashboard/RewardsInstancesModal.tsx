import { Flex, Text } from '@chakra-ui/react';
import React from 'react';

import type { GetInstancesResponse } from '@blockscout/points-types';

import { DialogBody, DialogContent, DialogRoot, DialogHeader } from 'toolkit/chakra/dialog';
import { Image } from 'toolkit/chakra/image';
import { Link } from 'toolkit/chakra/link';
import IconSvg from 'ui/shared/IconSvg';

type Props = {
  isOpen: boolean;
  onClose: () => void;
  items: GetInstancesResponse['items'] | undefined;
};

const RewardsInstancesModal = ({ isOpen, onClose, items }: Props) => {
  const handleOpenChange = React.useCallback(({ open }: { open: boolean }) => {
    if (!open) {
      onClose();
    }
  }, [ onClose ]);

  return (
    <DialogRoot
      open={ isOpen }
      onOpenChange={ handleOpenChange }
      size={{ lgDown: 'full', lg: 'sm' }}
    >
      <DialogContent>
        <DialogHeader>
          Choose explorer
        </DialogHeader>
        <DialogBody>
          <Flex flexDir="column" gap={ 6 }>
            <Text>
              Choose Blockscout explorer that you want to interact with and earn
              Merits
            </Text>
            <Flex flexWrap="wrap" gap={ 2 }>
              { items?.map((instance) => (
                <Link
                  external
                  noIcon
                  key={ instance.chain_id }
                  href={ instance.domain }
                  display="flex"
                  gap={ 2 }
                  alignItems="center"
                  p={ 2 }
                  bgColor={{ _light: 'blackAlpha.50', _dark: 'whiteAlpha.100' }}
                  borderRadius="base"
                >
                  <Image
                    src={ instance.details?.icon_url }
                    alt={ instance.name }
                    boxSize={ 5 }
                    flexShrink={ 0 }
                    fallback={ (
                      <IconSvg
                        name="networks/icon-placeholder"
                        color="icon.primary"
                      />
                    ) }
                  />
                  <Text
                    textStyle="sm"
                    fontWeight="500"
                    color="text.primary"
                    _groupHover={{ color: 'inherit' }}
                  >
                    { instance.name }
                  </Text>
                </Link>
              )) }
            </Flex>
          </Flex>
        </DialogBody>
      </DialogContent>
    </DialogRoot>
  );
};

export default RewardsInstancesModal;
