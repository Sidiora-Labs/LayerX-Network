import { Box, chakra } from '@chakra-ui/react';
import React from 'react';

import type { CeloEpochDetails } from 'types/api/epochs';

import useIsMobile from 'lib/hooks/useIsMobile';
import * as DetailedInfo from 'ui/shared/DetailedInfo/DetailedInfo';
import DetailedInfoTimestamp from 'ui/shared/DetailedInfo/DetailedInfoTimestamp';
import BlockEntity from 'ui/shared/entities/block/BlockEntity';
import CeloEpochStatus from 'ui/shared/statusTag/CeloEpochStatus';
import TokenTransferSnippet from 'ui/shared/TokenTransferSnippet/TokenTransferSnippet';
import NativeCoinValue from 'ui/shared/value/NativeCoinValue';
import TokenValue from 'ui/shared/value/TokenValue';

import EpochElectionRewards from './electionRewards/EpochElectionRewards';

interface Props {
  data: CeloEpochDetails;
  isLoading?: boolean;
}

const EpochDetails = ({ data, isLoading }: Props) => {
  const isMobile = useIsMobile();

  const processingRange = (() => {
    if (!data.start_processing_block_number || !data.end_processing_block_number) {
      return <Box color="text.secondary">N/A</Box>;
    }

    if (data.start_processing_block_number === data.end_processing_block_number) {
      return <BlockEntity number={ data.start_processing_block_number } isLoading={ isLoading } noIcon/>;
    }

    return (
      <>
        <BlockEntity number={ data.start_processing_block_number } isLoading={ isLoading } noIcon/>
        <chakra.span color="text.secondary" whiteSpace="pre"> - </chakra.span>
        <BlockEntity number={ data.end_processing_block_number } isLoading={ isLoading } noIcon/>
      </>
    );
  })();

  const totalFundRewards = (() => {
    if (!data.distribution?.transfers_total?.total?.value) {
      return <Box color="text.secondary">N/A</Box>;
    }

    if (data.distribution?.transfers_total?.token) {
      return (
        <TokenValue
          amount={ data.distribution?.transfers_total?.total?.value }
          token={ data.distribution?.transfers_total.token }
          decimals={ data.distribution?.transfers_total?.total?.decimals }
          accuracy={ 0 }
          loading={ isLoading }
        />
      );
    }

    return (
      <NativeCoinValue
        amount={ data.distribution?.transfers_total?.total?.value }
        accuracy={ 0 }
        loading={ isLoading }
      />
    );
  })();

  return (
    <>
      <DetailedInfo.Container>
        <DetailedInfo.ItemLabel
          hint="Current status of the epoch"
          isLoading={ isLoading }
        >
          Status
        </DetailedInfo.ItemLabel>
        <DetailedInfo.ItemValue>
          <CeloEpochStatus
            isFinalized={ data.is_finalized }
            loading={ isLoading }
          />
        </DetailedInfo.ItemValue>
        <DetailedInfo.ItemLabel
          hint="Timestamp of the block where the epoch processing completed"
          isLoading={ isLoading }
        >
          Timestamp
        </DetailedInfo.ItemLabel>
        <DetailedInfo.ItemValue>
          { data.timestamp ?
            <DetailedInfoTimestamp timestamp={ data.timestamp } isLoading={ isLoading }/> :
            <Box color="text.secondary" whiteSpace="pre-wrap">Epochs are finalized approximately once a day</Box> }
        </DetailedInfo.ItemValue>
        <DetailedInfo.ItemLabel
        // eslint-disable-next-line max-len
          hint={ `The range of blocks during which the epoch is processed — i.e., from the block where the "EpochProcessingStarted" event is emitted to the block where the "EpochProcessingEnded" event is emitted` }
          isLoading={ isLoading }
        >
          Processing range
        </DetailedInfo.ItemLabel>
        <DetailedInfo.ItemValue>
          { processingRange }
        </DetailedInfo.ItemValue>
        <DetailedInfo.ItemLabel
          hint="Funds allocation to support Celo projects and community initiatives"
          isLoading={ isLoading }
        >
          Community fund
        </DetailedInfo.ItemLabel>
        <DetailedInfo.ItemValue multiRow>
          { data.distribution?.community_transfer ? (
            <TokenTransferSnippet
              data={ data.distribution.community_transfer }
              isLoading={ isLoading }
              noAddressIcons={ isMobile }
            />
          ) : (
            <Box color="text.secondary">N/A</Box>
          ) }
        </DetailedInfo.ItemValue>
        <DetailedInfo.ItemLabel
          hint="Funds allocation to support projects that make Celo carbon-negative"
          isLoading={ isLoading }
        >
          Carbon offset fund
        </DetailedInfo.ItemLabel>
        <DetailedInfo.ItemValue multiRow>
          { data.distribution?.carbon_offsetting_transfer ? (
            <TokenTransferSnippet
              data={ data.distribution.carbon_offsetting_transfer }
              isLoading={ isLoading }
              noAddressIcons={ isMobile }
            />
          ) : (
            <Box color="text.secondary">N/A</Box>
          ) }
        </DetailedInfo.ItemValue>
        <DetailedInfo.ItemLabel
          hint="Sum of all fund allocations"
          isLoading={ isLoading }
        >
          Total fund rewards
        </DetailedInfo.ItemLabel>
        <DetailedInfo.ItemValue>
          { totalFundRewards }
        </DetailedInfo.ItemValue>
      </DetailedInfo.Container>
      <EpochElectionRewards data={ data } isLoading={ isLoading }/>
    </>
  );
};

export default React.memo(EpochDetails);
