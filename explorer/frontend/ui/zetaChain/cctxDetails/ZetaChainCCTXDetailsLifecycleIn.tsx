import { Flex, Grid, Text } from '@chakra-ui/react';
import React from 'react';

import type { CrossChainTx } from '@blockscout/zetachain-cctx-types';
import { InboundStatus } from '@blockscout/zetachain-cctx-types';

import config from 'configs/app';
import { Skeleton } from 'toolkit/chakra/skeleton';
import AddressEntityZetaChain from 'ui/shared/entities/address/AddressEntityZetaChain';
import TxEntity from 'ui/shared/entities/tx/TxEntity';
import TxEntityZetaChainCC from 'ui/shared/entities/tx/TxEntityZetaChainCC';
import TxEntityZetaChainExternal from 'ui/shared/entities/tx/TxEntityZetaChainExternal';
import IconSvg from 'ui/shared/IconSvg';
import StatusTag from 'ui/shared/statusTag/StatusTag';
import ZetaChainCCTXValue from 'ui/shared/zetaChain/ZetaChainCCTXValue';
import useZetaChainConfig from 'ui/zetaChain/useZetaChainConfig';

type Props = {
  tx: CrossChainTx;
  isLoading: boolean;
};

const ZetaChainCCTXDetailsLifecycleIn = ({ tx, isLoading }: Props) => {
  const { data: chainsConfig } = useZetaChainConfig();
  const inboundParams = tx.inbound_params;
  if (!inboundParams) {
    return null;
  }
  const chainFromId = inboundParams.sender_chain_id.toString();
  const chainFrom = chainsConfig?.find((chain) => chain.id.toString() === chainFromId);

  const isCCTX = tx.related_cctxs.some((cctx) => cctx.index === inboundParams.observed_hash);
  const color = inboundParams.status === InboundStatus.INBOUND_SUCCESS ? 'text.success' : 'text.error';

  return (
    <>
      <IconSvg
        name="verification-steps/finalized"
        boxSize={ 5 }
        bg="bg.primary"
        zIndex={ 1 }
        color={ color }
      />
      <Skeleton loading={ isLoading }>
        <Flex color={ color } maxH="20px" mb={ 2.5 } alignItems="center">
          { `Sender tx from ${ chainFrom?.name || 'unknown chain' }` }
        </Flex>
        <Grid
          templateColumns="100px 1fr"
          gap={ 3 }
          bg={{ _light: 'blackAlpha.100', _dark: 'whiteAlpha.100' }}
          py={ 3 }
          px={ 4 }
          borderBottomRadius="md"
          overflow="hidden"
          fontSize="sm"
        >
          { isCCTX ? (
            <>
              <Text color="text.secondary" fontWeight="medium">CCTX</Text>
              <TxEntityZetaChainCC hash={ inboundParams.observed_hash } isLoading={ isLoading } noIcon/>
            </>
          ) : (
            <>
              <Text color="text.secondary" fontWeight="medium">Transaction</Text>
              { chainFromId !== config.chain.id ? (
                <TxEntityZetaChainExternal chainId={ chainFromId } hash={ inboundParams.observed_hash } noIcon/>
              ) : (
                <TxEntity hash={ inboundParams.observed_hash } noIcon/>
              ) }
            </>
          ) }
          <Text color="text.secondary" fontWeight="medium">Status</Text>
          <StatusTag
            type={ inboundParams.status === InboundStatus.INBOUND_SUCCESS ? 'ok' : 'error' }
            text={ inboundParams.status === InboundStatus.INBOUND_SUCCESS ? 'Success' : 'Failed' }
          />
          { inboundParams.sender && (
            <>
              <Text color="text.secondary" fontWeight="medium">Sender</Text>
              <AddressEntityZetaChain
                address={{ hash: inboundParams.sender }}
                chainId={ inboundParams.sender_chain_id.toString() }
                isLoading={ isLoading }
                truncation="constant"
              />
            </>
          ) }
          { inboundParams.amount && (
            <>
              <Text color="text.secondary" fontWeight="medium">Transferred</Text>
              <ZetaChainCCTXValue
                coinType={ inboundParams.coin_type }
                tokenSymbol={ tx.token_symbol }
                amount={ inboundParams.amount }
                decimals={ tx.decimals ?? null }
                isLoading={ isLoading }
              />
            </>
          ) }
        </Grid>
      </Skeleton>
    </>
  );
};

export default ZetaChainCCTXDetailsLifecycleIn;
