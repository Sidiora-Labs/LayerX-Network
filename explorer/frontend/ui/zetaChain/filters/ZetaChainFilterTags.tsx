import { Flex, HStack, Text } from '@chakra-ui/react';
import { castArray } from 'es-toolkit/compat';
import React from 'react';

import type { ZetaChainCCTXFilterParams } from 'types/client/zetaChain';

import config from 'configs/app';
import dayjs from 'lib/date/dayjs';
import shortenString from 'lib/shortenString';
import { Link } from 'toolkit/chakra/link';
import { Tag } from 'toolkit/chakra/tag';
import { SECOND } from 'toolkit/utils/consts';
import IconSvg from 'ui/shared/IconSvg';
import useZetaChainConfig from 'ui/zetaChain/useZetaChainConfig';

type Props = {
  filters: ZetaChainCCTXFilterParams;
  onClearFilter: (key: keyof ZetaChainCCTXFilterParams) => () => void;
  onClearAll: () => void;
};

const ZetaChainFilterTags = ({ filters, onClearFilter, onClearAll }: Props) => {
  const { data: chains = [] } = useZetaChainConfig();

  const filterTags: Array<{ key: keyof ZetaChainCCTXFilterParams; name: string; value: string }> = [];

  // Age filter
  if (filters.age) {
    filterTags.push({
      key: 'age',
      name: 'Age',
      value: filters.age,
    });
  } else {
    if (filters.start_timestamp) {
      filterTags.push({
        key: 'start_timestamp',
        name: 'Date from',
        value: dayjs(Number(filters.start_timestamp) * SECOND).format('MMM DD, YYYY'),
      });
    }

    if (filters.end_timestamp) {
      filterTags.push({
        key: 'end_timestamp',
        name: 'Date to',
        value: dayjs(Number(filters.end_timestamp) * SECOND).format('MMM DD, YYYY'),
      });
    }
  }

  // Status filter
  const statusReduced = filters.status_reduced ? castArray(filters.status_reduced) : [];
  if (statusReduced.length > 0) {
    filterTags.push({
      key: 'status_reduced',
      name: 'Status',
      value: statusReduced.join(', '),
    });
  }

  // Sender filter
  const senderAddresses = filters.sender_address ? castArray(filters.sender_address) : [];
  if (senderAddresses.length > 0) {
    filterTags.push({
      key: 'sender_address',
      name: 'Sender',
      value: senderAddresses.map(address => shortenString(address, 8)).join(', '),
    });
  }

  // Sender chain filter
  const sourceChainIds = filters.source_chain_id ? castArray(filters.source_chain_id) : [];
  if (sourceChainIds.length > 0) {
    const chainNames = sourceChainIds.map(chainId => {
      const chain = chains.find(c => c.id.toString() === chainId);
      return chain?.name;
    });
    filterTags.push({
      key: 'source_chain_id',
      name: 'Sender Chain',
      value: chainNames.join(', '),
    });
  }

  // Receiver filter
  const receiverAddresses = filters.receiver_address ? castArray(filters.receiver_address) : [];
  if (receiverAddresses.length > 0) {
    filterTags.push({
      key: 'receiver_address',
      name: 'Receiver',
      value: receiverAddresses.map(address => shortenString(address, 8)).join(', '),
    });
  }

  // Receiver chain filter
  const targetChainIds = filters.target_chain_id ? castArray(filters.target_chain_id) : [];
  if (targetChainIds.length > 0) {
    const chainNames = targetChainIds.map(chainId => {
      const chain = chains.find(c => c.id.toString() === chainId);
      return chain?.name;
    });
    filterTags.push({
      key: 'target_chain_id',
      name: 'Receiver Chain',
      value: chainNames.join(', '),
    });
  }

  // Asset filter (coin type is used to filter by zeta native token)
  const tokenSymbols = filters.token_symbol ? castArray(filters.token_symbol) : [];
  const coinTypes = filters.coin_type ? castArray(filters.coin_type) : [];
  if (tokenSymbols.length > 0 || coinTypes.length > 0) {
    const hasZetaNativeToken = coinTypes.includes('Zeta');
    const value = [];
    if (hasZetaNativeToken) {
      value.push(config.chain.currency.symbol);
    }
    if (tokenSymbols.length > 0) {
      value.push(...tokenSymbols);
    }
    filterTags.push({
      key: 'token_symbol',
      name: 'Asset',
      value: value.join(', '),
    });
  }

  if (filterTags.length === 0) {
    return null;
  }

  return (
    <Flex justifyContent="space-between" alignItems="center" mb={ 6 }>
      <Flex alignItems="center" gap={ 2 }>
        <Text fontSize="lg" lineHeight="24px" w="100px">Filtered by:</Text>
        <HStack gap={ 2 } flexWrap="wrap">
          { filterTags.map(t => (
            <Tag key={ t.name } variant="filter" onClose={ onClearFilter(t.key) } closable label={ t.name }>
              { t.value }
            </Tag>
          )) }
        </HStack>
      </Flex>
      { filterTags.length !== 0 && (
        <Link onClick={ onClearAll } display="flex" alignItems="center" justifyContent="end" gap={ 2 } fontSize="sm" w="150px">
          <IconSvg name="repeat" boxSize={ 5 }/>
          Reset filters
        </Link>
      ) }
    </Flex>
  );
};

export default ZetaChainFilterTags;
