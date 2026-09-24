import { Box } from '@chakra-ui/react';
import React, { useMemo } from 'react';

import type { NovesResponseData } from 'types/api/noves';

import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { TableCell, TableRow } from 'toolkit/chakra/table';
import { SECOND } from 'toolkit/utils/consts';
import IconSvg from 'ui/shared/IconSvg';
import NovesFromTo from 'ui/shared/Noves/NovesFromTo';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';

type Props = {
  isPlaceholderData: boolean;
  tx: NovesResponseData;
  currentAddress: string;
};

const AddressAccountHistoryTableItem = (props: Props) => {

  const parsedDescription = useMemo(() => {
    const description = props.tx.classificationData.description;

    return description.endsWith('.') ? description.substring(0, description.length - 1) : description;
  }, [ props.tx.classificationData.description ]);

  return (
    <TableRow>
      <TableCell px={ 3 } py="18px" fontSize="sm" >
        <TimeWithTooltip
          timestamp={ props.tx.rawTransactionData.timestamp * SECOND }
          isLoading={ props.isPlaceholderData }
          color="text.secondary"
          borderRadius="sm"
          flexShrink={ 0 }
        />
      </TableCell>
      <TableCell px={ 3 } py="18px" fontSize="sm" >
        <Skeleton borderRadius="sm" loading={ props.isPlaceholderData }>
          <Box display="flex">
            <IconSvg
              name="lightning"
              height="5"
              width="5"
              color="icon.primary"
              mr="8px"
            />

            <Link
              href={ `/tx/${ props.tx.rawTransactionData.transactionHash }` }
              fontWeight="bold"
              whiteSpace="break-spaces"
              wordBreak="break-word"
            >
              { parsedDescription }
            </Link>
          </Box>
        </Skeleton>
      </TableCell>
      <TableCell px={ 3 } py="18px" fontSize="sm">
        <Box flexShrink={ 0 } >
          <NovesFromTo txData={ props.tx } currentAddress={ props.currentAddress } isLoaded={ !props.isPlaceholderData }/>
        </Box>
      </TableCell>
    </TableRow>
  );
};

export default React.memo(AddressAccountHistoryTableItem);
