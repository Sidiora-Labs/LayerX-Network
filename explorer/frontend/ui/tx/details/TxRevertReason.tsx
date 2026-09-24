import { Grid, GridItem, Text } from '@chakra-ui/react';
import React from 'react';

import type { TransactionRevertReason } from 'types/api/transaction';

import hexToUtf8 from 'lib/hexToUtf8';
import { HEX_REGEXP } from 'toolkit/utils/regexp';
import LogDecodedInputData from 'ui/shared/logs/LogDecodedInputData';

type Props = TransactionRevertReason;

const TxRevertReason = (props: Props) => {
  if ('raw' in props) {
    if (!HEX_REGEXP.test(props.raw)) {
      return <Text>{ props.raw }</Text>;
    }

    const decoded = hexToUtf8(props.raw);

    return (
      <Grid
        bgColor={{ _light: 'blackAlpha.50', _dark: 'whiteAlpha.50' }}
        p={ 4 }
        fontSize="sm"
        borderRadius="md"
        templateColumns="auto minmax(0, 1fr)"
        rowGap={ 2 }
        columnGap={ 4 }
        whiteSpace="normal"
      >
        <GridItem fontWeight={ 500 }>Raw:</GridItem>
        <GridItem>{ props.raw }</GridItem>
        { decoded.replace(/\s|\0/g, '') && (
          <>
            <GridItem fontWeight={ 500 }>Decoded:</GridItem>
            <GridItem>{ decoded }</GridItem>
          </>
        ) }
      </Grid>
    );
  }

  return <LogDecodedInputData data={ props }/>;
};

export default React.memo(TxRevertReason);
