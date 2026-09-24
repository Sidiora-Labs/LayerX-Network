import { chakra, Grid, HStack, type JsxStyleProps } from '@chakra-ui/react';
import React from 'react';

import type { InterchainTransfer } from '@blockscout/interchain-indexer-types';

import config from 'configs/app';
import dayjs from 'lib/date/dayjs';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { mdash } from 'toolkit/utils/htmlEntities';
import CrossChainBridgeLink from 'ui/shared/crossChain/CrossChainBridgeLink';
import CrossChainFromToTag from 'ui/shared/crossChain/CrossChainFromToTag';
import AddressEntityInterchain from 'ui/shared/entities/address/AddressEntityInterchain';
import CrossChainMessageEntity from 'ui/shared/entities/crossChainMessage/CrossChainMessageEntity';
import TxEntityInterchain from 'ui/shared/entities/tx/TxEntityInterchain';
import ListItemMobile from 'ui/shared/ListItemMobile/ListItemMobile';
import CrossChainTxsStatusTag from 'ui/shared/statusTag/CrossChainTxsStatusTag';
import TextSeparator from 'ui/shared/TextSeparator';
import Time from 'ui/shared/time/Time';
import TokenValueInterchain from 'ui/shared/value/TokenValueInterchain';

interface Props extends JsxStyleProps {
  data: InterchainTransfer;
  isLoading?: boolean;
  currentAddress?: string;
}

const TokenTransfersCrossChainListItem = ({ data, isLoading, rowGap = 3, currentAddress, ...rest }: Props) => {

  const timestamp = data.send_timestamp || data.receive_timestamp;

  const dashElement = <chakra.span color="text.secondary">{ mdash }</chakra.span>;

  return (
    <ListItemMobile rowGap={ rowGap } { ...rest }>
      <HStack>
        <CrossChainTxsStatusTag status={ data.status } loading={ isLoading } mode="full"/>
        { currentAddress && (
          <CrossChainFromToTag
            type={ data.sender?.hash.toLowerCase() === currentAddress.toLowerCase() && config.chain.id === data.source_chain?.id ? 'out' : 'in' }
            isLoading={ isLoading }
          />
        ) }
      </HStack>
      { timestamp && (
        <Skeleton loading={ isLoading } display="flex" alignItems="center" color="text.secondary">
          <div>{ dayjs(timestamp).fromNow() }</div>
          <TextSeparator/>
          <Time timestamp={ timestamp } format="lll_s"/>
        </Skeleton>
      ) }
      <Grid templateColumns="100px minmax(0, 1fr)" columnGap={ 2 } rowGap={ rowGap }>
        <Skeleton loading={ isLoading }>
          Source token
        </Skeleton>
        { data.source_token ? (
          <TokenValueInterchain
            token={ data.source_token }
            amount={ data.source_amount }
            chain={ data.source_chain }
            loading={ isLoading }
          />
        ) : dashElement }
        <Skeleton loading={ isLoading }>
          Sender
        </Skeleton>
        { data.sender ? (
          <AddressEntityInterchain
            chain={ data.source_chain }
            address={ data.sender }
            isLoading={ isLoading }
            noIcon
            currentAddress={ currentAddress }
          />
        ) : dashElement }
        <Skeleton loading={ isLoading }>
          Target token
        </Skeleton>
        { data.destination_token ? (
          <TokenValueInterchain
            token={ data.destination_token }
            amount={ data.destination_amount }
            chain={ data.destination_chain }
            loading={ isLoading }
          />
        ) : dashElement }
        <Skeleton loading={ isLoading }>
          Recipient
        </Skeleton>
        { data.recipient ? (
          <AddressEntityInterchain
            chain={ data.destination_chain }
            address={ data.recipient }
            isLoading={ isLoading }
            noIcon
            currentAddress={ currentAddress }
          />
        ) : dashElement }
        <Skeleton loading={ isLoading }>
          Source tx
        </Skeleton>
        { data.source_transaction_hash ? (
          <TxEntityInterchain
            chain={ data.source_chain }
            hash={ data.source_transaction_hash }
            isLoading={ isLoading }
          />
        ) : dashElement }
        <Skeleton loading={ isLoading }>
          Destination tx
        </Skeleton>
        { data.destination_transaction_hash ? (
          <TxEntityInterchain
            chain={ data.destination_chain }
            hash={ data.destination_transaction_hash }
            isLoading={ isLoading }
          />
        ) : dashElement }
        <Skeleton loading={ isLoading }>
          Protocol
        </Skeleton>
        { data.bridge ? (
          <CrossChainBridgeLink data={ data.bridge } isLoading={ isLoading }/>
        ) : dashElement }
        <Skeleton loading={ isLoading }>
          Message
        </Skeleton>
        { data.message_id ? (
          <CrossChainMessageEntity id={ data.message_id } isLoading={ isLoading }/>
        ) : dashElement }
      </Grid>
    </ListItemMobile>
  );
};

export default React.memo(TokenTransfersCrossChainListItem);
