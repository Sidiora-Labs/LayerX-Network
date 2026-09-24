import { Box, Text } from '@chakra-ui/react';
import React from 'react';

import { Link } from 'toolkit/chakra/link';

const EthRpcApi = () => {
  return (
    <Box>
      <Text>
        In addition to the custom RPC endpoints documented here,
        the Blockscout ETH RPC API supports 3 methods in the exact format specified for Ethereum nodes,
        ee the Ethereum JSON-RPC Specification for more details.
      </Text>
      <Link href="https://docs.blockscout.com/devs/apis/rpc/eth-rpc" external mt={ 6 }>View examples</Link>
    </Box>
  );
};

export default React.memo(EthRpcApi);
