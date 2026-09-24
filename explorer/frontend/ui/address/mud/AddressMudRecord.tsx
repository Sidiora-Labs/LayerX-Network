import { Box, Flex, Separator, Text, VStack } from '@chakra-ui/react';
import { useRouter } from 'next/router';
import React from 'react';

import useApiQuery from 'lib/api/useApiQuery';
import getQueryParamString from 'lib/router/getQueryParamString';
import { TableRoot, TableRow, TableCell } from 'toolkit/chakra/table';
import { ContentLoader } from 'toolkit/components/loaders/ContentLoader';
import { TruncatedText } from 'toolkit/components/truncation/TruncatedText';
import DataFetchAlert from 'ui/shared/DataFetchAlert';
import Time from 'ui/shared/time/Time';

import AddressMudBreadcrumbs from './AddressMudBreadcrumbs';
import AddressMudRecordValues from './AddressMudRecordValues';
import { getValueString } from './utils';

type Props = {
  isQueryEnabled?: boolean;
  tableId: string;
  recordId: string;
};

const AddressMudRecord = ({ tableId, recordId, isQueryEnabled = true }: Props) => {
  const router = useRouter();

  const hash = getQueryParamString(router.query.hash);

  const { data, isLoading, isError } = useApiQuery('general:mud_record', {
    pathParams: { hash, table_id: tableId, record_id: recordId },
    queryOptions: {
      enabled: isQueryEnabled,
    },
  });

  if (isLoading) {
    return <ContentLoader/>;
  }

  if (isError) {
    return <DataFetchAlert/>;
  }

  return (
    <>
      { data && (
        <AddressMudBreadcrumbs
          hash={ hash }
          tableId={ tableId }
          tableName={ data?.table.table_full_name }
          recordId={ recordId }
          recordName={ data.record.id }
          mb={ 6 }
        />
      ) }
      <Box hideBelow="lg">
        <TableRoot borderRadius="8px" style={{ tableLayout: 'auto' }} width="100%" overflow="hidden">
          { data?.schema.key_names.length && data?.schema.key_names.map((keyName, index) => (
            <TableRow key={ keyName } borderBottomStyle={ index === data.schema.key_names.length - 1 ? 'hidden' : 'solid' }>
              <TableCell fontWeight={ 600 } whiteSpace="nowrap" fontSize="sm">
                { keyName } ({ data.schema.key_types[index] })
              </TableCell>
              <TableCell colSpan={ 2 } fontSize="sm">
                <Flex justifyContent="space-between">
                  <TruncatedText text={ getValueString(data.record.decoded[keyName]) } mr={ 2 }/>
                  { index === 0 && <Time color="text.secondary" timestamp={ data.record.timestamp }/> }
                </Flex>
              </TableCell>
            </TableRow>
          )) }
          <AddressMudRecordValues data={ data }/>
        </TableRoot>
      </Box>
      <Box hideFrom="lg">
        <>
          { data?.schema.key_names.length && data?.schema.key_names.map((keyName, index) => (
            <VStack gap={ 1 } key={ keyName } alignItems="start" fontSize="sm">
              <Separator/>
              <Text fontWeight={ 600 } whiteSpace="nowrap">
                { keyName } ({ data.schema.key_types[index] })
              </Text>
              <Text wordBreak="break-word">{ getValueString(data.record.decoded[keyName]) }</Text>
              { index === 0 && <Time color="text.secondary" timestamp={ data.record.timestamp }/> }
            </VStack>
          )) }
          <TableRoot borderRadius="8px" style={{ tableLayout: 'auto' }} width="100%" mt={ 2 } overflow="hidden">
            <AddressMudRecordValues data={ data }/>
          </TableRoot>
        </>
      </Box>
    </>
  );
};

export default AddressMudRecord;
