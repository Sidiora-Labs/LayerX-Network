import { Text } from '@chakra-ui/react';
import BigNumber from 'bignumber.js';
import React from 'react';

import type { Block } from 'types/api/block';

import { currencyUnits } from 'lib/units';
import { Tooltip } from 'toolkit/chakra/tooltip';
import { ZERO } from 'toolkit/utils/consts';
import * as DetailedInfo from 'ui/shared/DetailedInfo/DetailedInfo';
import IconSvg from 'ui/shared/IconSvg';
import Utilization from 'ui/shared/Utilization/Utilization';
import GasPriceValue from 'ui/shared/value/GasPriceValue';
import NativeCoinValue from 'ui/shared/value/NativeCoinValue';

interface Props {
  data: Block;
}

const BlockDetailsBlobInfo = ({ data }: Props) => {
  if (
    !data.blob_gas_price ||
    !data.blob_gas_used ||
    !data.burnt_blob_fees ||
    !data.excess_blob_gas
  ) {
    return null;
  }

  const burntBlobFees = BigNumber(data.burnt_blob_fees || 0);
  const blobFees = BigNumber(data.blob_gas_price || 0).multipliedBy(BigNumber(data.blob_gas_used || 0));

  return (
    <>

      { data.blob_gas_price && (
        <>
          <DetailedInfo.ItemLabel
            // eslint-disable-next-line max-len
            hint="Price per unit of gas used for for blob deployment. Blob gas is independent of normal gas. Both gas prices can affect the priority of transaction execution."
          >
            Blob gas price
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue multiRow>
            <GasPriceValue amount={ data.blob_gas_price }/>
          </DetailedInfo.ItemValue>
        </>
      ) }
      { data.blob_gas_used && (
        <>
          <DetailedInfo.ItemLabel
            hint="Actual amount of gas used by the blobs in this block"
          >
            Blob gas used
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue>
            <Text>{ BigNumber(data.blob_gas_used).toFormat() }</Text>
          </DetailedInfo.ItemValue>
        </>
      ) }
      { !burntBlobFees.isEqualTo(ZERO) && (
        <>
          <DetailedInfo.ItemLabel
            hint={ `Amount of ${ currencyUnits.ether } used for blobs in this block` }
          >
            Blob burnt fees
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue multiRow>
            <NativeCoinValue
              amount={ burntBlobFees.toString() }
              accuracy={ 0 }
              startElement={ <IconSvg name="flame" boxSize={ 5 } color="icon.primary" mr={{ base: 1, lg: 2 }}/> }
              mr={ 4 }
            />
            { !blobFees.isEqualTo(ZERO) && (
              <Tooltip content="Blob burnt fees / Txn fees * 100%">
                <Utilization value={ burntBlobFees.dividedBy(blobFees).toNumber() }/>
              </Tooltip>
            ) }
          </DetailedInfo.ItemValue>
        </>
      ) }
      { data.excess_blob_gas && (
        <>
          <DetailedInfo.ItemLabel
            hint="A running total of blob gas consumed in excess of the target, prior to the block."
          >
            Excess blob gas
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue>
            <GasPriceValue amount={ data.excess_blob_gas }/>
          </DetailedInfo.ItemValue>
        </>
      ) }
      <DetailedInfo.ItemDivider/>
    </>
  );
};

export default React.memo(BlockDetailsBlobInfo);
