import { Box } from '@chakra-ui/react';
import React from 'react';

import type { AddressMudRecord } from 'types/api/address';

import { TableCell, TableRow } from 'toolkit/chakra/table';

import { getValueString } from './utils';

type Props = {
  data?: AddressMudRecord;
};

const AddressMudRecordValues = ({ data }: Props) => {
  const valuesBgColor = { _light: 'blackAlpha.50', _dark: 'whiteAlpha.50' };

  if (!data?.schema.value_names.length) {
    return null;
  }

  return (
    <>
      <TableRow backgroundColor={ valuesBgColor } borderBottomStyle="hidden" >
        <TableCell fontWeight={ 600 } w="100px" fontSize="sm">Field</TableCell>
        <TableCell fontWeight={ 600 } w="90px" fontSize="sm">Type</TableCell>
        <TableCell fontWeight={ 600 } fontSize="sm">Value</TableCell>
      </TableRow>
      {
        data?.schema.value_names.map((valName, index) => (
          <TableRow key={ valName } backgroundColor={ valuesBgColor } borderBottomStyle="hidden">
            <TableCell fontWeight={ 400 } w="100px" py={ 0 } pb={ 4 } pr={ 0 } wordBreak="break-all">{ valName }</TableCell>
            <TableCell fontWeight={ 400 } w="90px" py={ 0 } pb={ 4 } wordBreak="break-all">{ data.schema.value_types[index] }</TableCell>
            <TableCell fontWeight={ 400 } wordBreak="break-word" py={ 0 } pb={ 4 }>
              <Box>
                { getValueString(data.record.decoded[valName]) }
              </Box>
            </TableCell>
          </TableRow>
        ))
      }
    </>
  );
};

export default AddressMudRecordValues;
