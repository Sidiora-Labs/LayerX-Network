import { Box, Flex, Text } from '@chakra-ui/react';
import React, { useMemo } from 'react';

import type { NovesResponseData } from 'types/api/noves';

import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { SECOND } from 'toolkit/utils/consts';
import IconSvg from 'ui/shared/IconSvg';
import ListItemMobile from 'ui/shared/ListItemMobile/ListItemMobile';
import NovesFromTo from 'ui/shared/Noves/NovesFromTo';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';

type Props = {
  isPlaceholderData: boolean;
  tx: NovesResponseData;
  currentAddress: string;
};

const AddressAccountHistoryListItem = (props: Props) => {

  const parsedDescription = useMemo(() => {
    const description = props.tx.classificationData.description;

    return description.endsWith('.') ? description.substring(0, description.length - 1) : description;
  }, [ props.tx.classificationData.description ]);

  return (
    <ListItemMobile rowGap={ 4 } w="full">
      <Skeleton borderRadius="sm" loading={ props.isPlaceholderData } w="full">
        <Flex justifyContent="space-between" w="full">
          <Flex columnGap={ 2 }>
            <IconSvg
              name="lightning"
              height="5"
              width="5"
              color="icon.primary"
            />

            <Text fontSize="sm" fontWeight={ 500 }>
              Action
            </Text>
          </Flex>
          <TimeWithTooltip
            timestamp={ props.tx.rawTransactionData.timestamp * SECOND }
            color="text.secondary"
            borderRadius="sm"
            fontWeight={ 500 }
          />
        </Flex>
      </Skeleton>
      <Skeleton borderRadius="sm" loading={ props.isPlaceholderData }>
        <Link
          href={ `/tx/${ props.tx.rawTransactionData.transactionHash }` }
          fontWeight="bold"
          whiteSpace="break-spaces"
          wordBreak="break-word"
        >
          { parsedDescription }
        </Link>
      </Skeleton>

      <Box maxW="full">
        <NovesFromTo txData={ props.tx } currentAddress={ props.currentAddress } isLoaded={ !props.isPlaceholderData }/>
      </Box>
    </ListItemMobile>
  );
};

export default React.memo(AddressAccountHistoryListItem);
