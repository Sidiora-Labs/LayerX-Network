import { Grid, GridItem } from '@chakra-ui/react';
import React from 'react';

import type { Log } from 'types/api/log';
import type { ClusterChainConfig } from 'types/multichain';

import { route } from 'nextjs-routes';

// import searchIcon from 'icons/search.svg';
import { Alert } from 'toolkit/chakra/alert';
import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { space } from 'toolkit/utils/htmlEntities';
import DetailedInfoTimestamp from 'ui/shared/DetailedInfo/DetailedInfoTimestamp';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import TxEntity from 'ui/shared/entities/tx/TxEntity';
import LogDecodedInputData from 'ui/shared/logs/LogDecodedInputData';
import LogTopic from 'ui/shared/logs/LogTopic';
import type { DataType } from 'ui/shared/RawInputData';
import RawInputData from 'ui/shared/RawInputData';

import LogIndex from './LogIndex';

type Props = Log & {
  type: 'address' | 'transaction';
  isLoading?: boolean;
  defaultDataType?: DataType;
  chainData?: ClusterChainConfig;
};

const RowHeader = ({ children, isLoading }: { children: React.ReactNode; isLoading?: boolean }) => (
  <GridItem _notFirst={{ my: { base: 4, lg: 0 } }}>
    <Skeleton fontWeight={ 500 } loading={ isLoading } display="inline-block">{ children }</Skeleton>
  </GridItem>
);

const LogItem = ({
  address,
  index,
  topics,
  data,
  decoded,
  type,
  transaction_hash: txHash,
  block_timestamp: blockTimestamp,
  isLoading,
  defaultDataType,
  chainData,
}: Props) => {

  const hasTxInfo = type === 'address' && txHash;

  return (
    <Grid
      gridTemplateColumns={{ base: 'minmax(0, 1fr)', lg: '200px minmax(0, 1fr)' }}
      gap={{ base: 2, lg: 8 }}
      py={ 8 }
      _notFirst={{
        borderTopWidth: '1px',
        borderTopColor: { _light: 'blackAlpha.200', _dark: 'whiteAlpha.200' },
      }}
      _first={{
        pt: 0,
      }}
    >
      { !decoded && !address.is_verified && type === 'transaction' && (
        <GridItem colSpan={{ base: 1, lg: 2 }}>
          <Alert status="warning" display="inline-table" whiteSpace="normal">
            To see accurate decoded input data, the contract must be verified.{ space }
            <Link href={ route({ pathname: '/address/[hash]/contract-verification', query: { hash: address.hash } }) }>Verify the contract here</Link>
          </Alert>
        </GridItem>
      ) }
      { hasTxInfo ? <RowHeader isLoading={ isLoading }>Transaction</RowHeader> : <RowHeader isLoading={ isLoading }>Address</RowHeader> }
      <GridItem display="flex" alignItems="center">
        { type === 'address' && txHash ? (
          <TxEntity
            hash={ txHash }
            isLoading={ isLoading }
            mr={{ base: 9, lg: 4 }}
            w="100%"
            chain={ chainData }
            noCopy
          />
        ) : (
          <AddressEntity
            address={ address }
            isLoading={ isLoading }
            mr={{ base: 9, lg: 4 }}
          />
        ) }
        { /* api doesn't have find topic feature yet */ }
        { /* <Tooltip label="Find matches topic">
          <Link ml={ 2 } mr={{ base: 9, lg: 0 }} display="inline-flex">
            <Icon as={ searchIcon } boxSize={ 5 }/>
          </Link>
        </Tooltip> */ }
        <LogIndex
          isLoading={ isLoading }
          textStyle="sm"
          ml="auto"
          minW={ 8 }
          height={ 8 }
        >
          { index }
        </LogIndex>
      </GridItem>
      { hasTxInfo && blockTimestamp ? (
        <>
          <RowHeader isLoading={ isLoading }>Timestamp</RowHeader>
          <GridItem>
            <DetailedInfoTimestamp timestamp={ blockTimestamp } isLoading={ isLoading }/>
          </GridItem>
        </>
      ) : null }
      { decoded && (
        <>
          <RowHeader isLoading={ isLoading }>Decode input data</RowHeader>
          <GridItem>
            <LogDecodedInputData data={ decoded } isLoading={ isLoading }/>
          </GridItem>
        </>
      ) }
      <RowHeader isLoading={ isLoading }>Topics</RowHeader>
      <GridItem>
        { topics.filter(Boolean).map((item, index) => (
          <LogTopic
            key={ index }
            hex={ item }
            index={ index }
            isLoading={ isLoading }
          />
        )) }
      </GridItem>
      <RowHeader isLoading={ isLoading }>Data</RowHeader>
      { defaultDataType ? (
        <RawInputData hex={ data } isLoading={ isLoading } defaultDataType={ defaultDataType } minHeight="53px"/>
      ) : (
        <Skeleton
          loading={ isLoading }
          p={ 4 }
          fontSize="sm"
          borderRadius="md"
          bgColor={ isLoading ? undefined : { _light: 'blackAlpha.50', _dark: 'whiteAlpha.50' } }
        >
          { data }
        </Skeleton>
      ) }
    </Grid>
  );
};

export default React.memo(LogItem);
