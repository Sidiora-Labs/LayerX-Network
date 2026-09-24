import { Flex, chakra } from '@chakra-ui/react';
import React from 'react';

import type { VerifiedContract } from 'types/api/contracts';
import type { ClusterChainConfig } from 'types/multichain';

import formatLanguageName from 'lib/contracts/formatLanguageName';
import { CONTRACT_LICENSES } from 'lib/contracts/licenses';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { TableCell, TableRow } from 'toolkit/chakra/table';
import { Tooltip } from 'toolkit/chakra/tooltip';
import ContractCertifiedLabel from 'ui/shared/ContractCertifiedLabel';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import ChainIcon from 'ui/shared/externalChains/ChainIcon';
import IconSvg from 'ui/shared/IconSvg';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';
import NativeCoinValue from 'ui/shared/value/NativeCoinValue';

interface Props {
  data: VerifiedContract;
  isLoading?: boolean;
  chainData?: ClusterChainConfig;
}

const VerifiedContractsTableItem = ({ data, isLoading, chainData }: Props) => {
  const license = (() => {
    const license = CONTRACT_LICENSES.find((license) => license.type === data.license_type);
    if (!license || license.type === 'none') {
      return '-';
    }

    return license.label;
  })();

  return (
    <TableRow>
      { chainData && (
        <TableCell>
          <ChainIcon data={ chainData } isLoading={ isLoading } mt={ 1 }/>
        </TableCell>
      ) }
      <TableCell>
        <Flex alignItems="center" mt={ 1 }>
          <AddressEntity
            address={ data.address }
            isLoading={ isLoading }
            query={{ tab: 'contract', ...(chainData ? { chain_id: chainData.id } : {}) }}
            noCopy
          />
          { data.certified && <ContractCertifiedLabel iconSize={ 5 } boxSize={ 5 } ml={ 2 }/> }
        </Flex>
        <AddressEntity
          address={{ hash: data.address.filecoin?.robust ?? data.address.hash }}
          isLoading={ isLoading }
          noLink
          noIcon
          truncation="constant"
          my={ 1 }
          ml={ 7 }
          color="text.secondary"
          w="fit-content"
        />
      </TableCell>
      <TableCell isNumeric>
        <NativeCoinValue
          amount={ data.coin_balance }
          noSymbol
          loading={ isLoading }
          my={ 1 }
        />
      </TableCell>
      <TableCell isNumeric>
        <Skeleton loading={ isLoading } display="inline-block" my={ 1 }>
          { data.transactions_count ? data.transactions_count.toLocaleString() : '0' }
        </Skeleton>
      </TableCell>
      <TableCell>
        <Flex flexWrap="wrap" columnGap={ 2 }>
          <Skeleton loading={ isLoading } my={ 1 }>{ formatLanguageName(data.language) }</Skeleton>
          { data.compiler_version && (
            <Skeleton loading={ isLoading } color="text.secondary" wordBreak="break-all" my={ 1 } cursor="pointer">
              <Tooltip content={ data.compiler_version }>
                <span>{ data.compiler_version.split('+')[0] }</span>
              </Tooltip>
            </Skeleton>
          ) }
        </Flex>
        { data.zk_compiler_version && (
          <Flex flexWrap="wrap" columnGap={ 2 } my={ 1 }>
            <Skeleton loading={ isLoading } >ZK compiler</Skeleton>
            <Skeleton loading={ isLoading } color="text.secondary" wordBreak="break-all">
              <span>{ data.zk_compiler_version }</span>
            </Skeleton>
          </Flex>
        ) }
      </TableCell>
      <TableCell>
        <Tooltip content="Optimization" disabled={ isLoading }>
          <chakra.span display="inline-block">
            { data.optimization_enabled ?
              <IconSvg name="check" boxSize={ 6 } color="green.500" cursor="pointer" isLoading={ isLoading }/> :
              <IconSvg name="cross" boxSize={ 6 } color="red.600" cursor="pointer" isLoading={ isLoading }/> }
          </chakra.span>
        </Tooltip>
        <Tooltip content="Constructor args" disabled={ isLoading }>
          <chakra.span display="inline-block" ml={ 2 }>
            { data.has_constructor_args ?
              <IconSvg name="check" boxSize={ 6 } color="green.500" cursor="pointer" isLoading={ isLoading }/> :
              <IconSvg name="cross" boxSize={ 6 } color="red.600" cursor="pointer" isLoading={ isLoading }/> }
          </chakra.span>
        </Tooltip>
      </TableCell>
      <TableCell>
        <Flex alignItems="center" columnGap={ 2 } my={ 1 }>
          <IconSvg name="status/success" boxSize={ 4 } color="green.500" isLoading={ isLoading }/>
          <TimeWithTooltip
            timestamp={ data.verified_at }
            isLoading={ isLoading }
            color="text.secondary"
          />
        </Flex>
      </TableCell>
      <TableCell>
        <Skeleton loading={ isLoading } my={ 1 } display="inline-block">
          { license }
        </Skeleton>
      </TableCell>
    </TableRow>
  );
};

export default React.memo(VerifiedContractsTableItem);
