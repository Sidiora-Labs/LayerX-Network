import { chakra, VStack } from '@chakra-ui/react';
import React from 'react';

import type { InterchainMessage } from '@blockscout/interchain-indexer-types';

import { route } from 'nextjs-routes';

import config from 'configs/app';
import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { TableCell, TableRow } from 'toolkit/chakra/table';
import { mdash } from 'toolkit/utils/htmlEntities';
import AddressFromToIcon from 'ui/shared/address/AddressFromToIcon';
import CrossChainBridgeLink from 'ui/shared/crossChain/CrossChainBridgeLink';
import CrossChainFromToTag from 'ui/shared/crossChain/CrossChainFromToTag';
import AddressEntityInterchain from 'ui/shared/entities/address/AddressEntityInterchain';
import CrossChainMessageEntity from 'ui/shared/entities/crossChainMessage/CrossChainMessageEntity';
import TxEntityInterchain from 'ui/shared/entities/tx/TxEntityInterchain';
import ChainLabel from 'ui/shared/externalChains/ChainLabel';
import CrossChainTxsStatusTag from 'ui/shared/statusTag/CrossChainTxsStatusTag';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';
import TokenValueInterchain from 'ui/shared/value/TokenValueInterchain';

interface Props {
  data: InterchainMessage;
  isLoading?: boolean;
  currentAddress?: string;
}

const TransactionsCrossChainTableItem = ({ data, isLoading, currentAddress }: Props) => {

  const firstTransfer = data.transfers.length > 0 ? data.transfers[0] : null;
  const txHashWithTransfers = (() => {
    if (data.transfers.length === 0) {
      return;
    }

    if (config.chain.id === data.source_chain?.id) {
      return data.source_transaction_hash;
    }

    if (config.chain.id === data.destination_chain?.id) {
      return data.destination_transaction_hash;
    }
  })();

  const dashElement = <chakra.span color="text.secondary" lineHeight="24px">{ mdash }</chakra.span>;

  return (
    <TableRow>
      <TableCell w="42px">
        <CrossChainTxsStatusTag status={ data.status } loading={ isLoading }/>
      </TableCell>
      { currentAddress && (
        <TableCell>
          <CrossChainFromToTag
            type={ data.sender?.hash.toLowerCase() === currentAddress.toLowerCase() && config.chain.id === data.source_chain?.id ? 'out' : 'in' }
            isLoading={ isLoading }
          />
        </TableCell>
      ) }
      <TableCell>
        <CrossChainMessageEntity id={ data.message_id } isLoading={ isLoading } lineHeight="24px" fontWeight={ 700 }/>
      </TableCell>
      <TableCell>
        <TimeWithTooltip
          timestamp={ data.send_timestamp || data.receive_timestamp }
          isLoading={ isLoading }
          enableIncrement
          color="text.secondary"
          lineHeight="24px"
          whiteSpace="nowrap"
        />
      </TableCell>
      <TableCell maxW="150px">
        { data.sender ? (
          <AddressEntityInterchain
            chain={ data.source_chain }
            address={ data.sender }
            isLoading={ isLoading }
            truncation="constant"
            noIcon
            currentAddress={ currentAddress }
            lineHeight="24px"
            maxW="100%"
            w="fit-content"
          />
        ) : dashElement }
      </TableCell>
      <TableCell maxW="150px">
        <VStack alignItems="start">
          { data.source_transaction_hash ? (
            <TxEntityInterchain
              chain={ data.source_chain }
              hash={ data.source_transaction_hash }
              isLoading={ isLoading }
              noIcon
              truncation="constant"
              lineHeight="24px"
            />
          ) : dashElement }
          <ChainLabel
            data={ data.source_chain }
            isLoading={ isLoading }
            color="text.secondary"
            textStyle="xs"
            gap={ 1 }
            fallback={ data.source_transaction_hash ? <chakra.span color="text.secondary">{ mdash }</chakra.span> : null }
          />
        </VStack>
      </TableCell>
      <TableCell maxW="150px">
        <VStack alignItems="start">
          { data.destination_transaction_hash ? (
            <TxEntityInterchain
              chain={ data.destination_chain }
              hash={ data.destination_transaction_hash }
              isLoading={ isLoading }
              noIcon
              truncation="constant"
              lineHeight="24px"
            />
          ) : dashElement }
          <ChainLabel
            data={ data.destination_chain }
            isLoading={ isLoading }
            color="text.secondary"
            textStyle="xs"
            gap={ 1 }
            fallback={ data.destination_transaction_hash ? <chakra.span color="text.secondary">{ mdash }</chakra.span> : null }
          />
        </VStack>
      </TableCell>
      <TableCell>
        { txHashWithTransfers ? (
          <Link
            href={ route({ pathname: '/tx/[hash]', query: { hash: txHashWithTransfers, tab: 'token_transfers_cross_chain' } }) }
            loading={ isLoading }
            lineHeight="24px"
            minW="24px"
          >
            { data.transfers.length }
          </Link>
        ) : (
          <Skeleton loading={ isLoading } color="text.secondary" lineHeight="24px">
            <span>
              { data.transfers.length }
            </span>
          </Skeleton>
        ) }
      </TableCell>
      <TableCell maxW="150px">
        <VStack alignItems="start">
          {
            firstTransfer?.sender ? (
              <AddressEntityInterchain
                chain={ firstTransfer.source_chain }
                address={ firstTransfer.sender }
                isLoading={ isLoading }
                truncation="constant"
                noIcon
                currentAddress={ currentAddress }
                lineHeight="24px"
                maxW="100%"
              />
            ) : dashElement
          }
          { firstTransfer?.source_token && (
            <TokenValueInterchain
              token={ firstTransfer.source_token }
              amount={ firstTransfer.source_amount }
              chain={ firstTransfer.source_chain }
              loading={ isLoading }
              textStyle="xs"
              color="text.secondary"
            />
          ) }
          { data.transfers.length > 1 && (
            <Link
              variant="secondary"
              textDecorationStyle="dashed"
              textDecorationLine="underline"
              mt={ 2 }
              href={ route({ pathname: '/cross-chain-tx/[id]', query: { id: data.message_id, tab: 'transfers' } }) }
              textStyle="xs"
            >
              View all
            </Link>
          ) }
        </VStack>
      </TableCell>
      <TableCell>
        <AddressFromToIcon type="unspecified" isLoading={ isLoading } mt={ 0.5 }/>
      </TableCell>
      <TableCell maxW="150px">
        <VStack alignItems="start">
          {
            firstTransfer?.recipient ? (
              <AddressEntityInterchain
                chain={ firstTransfer.destination_chain }
                address={ firstTransfer.recipient }
                isLoading={ isLoading }
                truncation="constant"
                noIcon
                currentAddress={ currentAddress }
                lineHeight="24px"
                maxW="100%"
              />
            ) : dashElement
          }
          { firstTransfer?.destination_token && (
            <TokenValueInterchain
              token={ firstTransfer.destination_token }
              amount={ firstTransfer.destination_amount }
              chain={ firstTransfer.destination_chain }
              loading={ isLoading }
              textStyle="xs"
              color="text.secondary"
            />
          ) }
        </VStack>
      </TableCell>
      <TableCell>
        <CrossChainBridgeLink data={ data.bridge } isLoading={ isLoading } lineHeight="24px"/>
      </TableCell>
    </TableRow>
  );
};

export default React.memo(TransactionsCrossChainTableItem);
