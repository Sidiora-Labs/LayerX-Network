import { Text } from '@chakra-ui/react';
import React from 'react';

import type { PaxeerXReceipt, PaxeerXVerificationStatus } from 'types/api/paxeerXLists';

import { Skeleton } from 'toolkit/chakra/skeleton';
import CopyToClipboard from 'ui/shared/CopyToClipboard';
import * as DetailedInfo from 'ui/shared/DetailedInfo/DetailedInfo';
import BlockEntity from 'ui/shared/entities/block/BlockEntity';
import TxEntity from 'ui/shared/entities/tx/TxEntity';
import StatusLadderBadge from 'ui/shared/statusLadder/StatusLadderBadge';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';

export const VERIFICATION_STATUS_LABELS: Record<PaxeerXVerificationStatus, string> = {
  unverified: 'Unverified',
  sequencer_signed: 'Sequencer signed',
  batch_included: 'Batch included',
  state_proven: 'State proven',
  checkpoint_finalised: 'Checkpoint finalised',
  settlement_anchored: 'Settlement anchored',
};

interface Props {
  data: PaxeerXReceipt;
  isLoading?: boolean;
}

const PaxeerXReceiptDetails = ({ data, isLoading }: Props) => {
  return (
    <DetailedInfo.Container>
      <DetailedInfo.ItemLabel isLoading={ isLoading }>Receipt ID</DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue overflow="hidden">
        <Skeleton loading={ isLoading } overflow="hidden" textOverflow="ellipsis" data-field="id">
          { data.id }
        </Skeleton>
        <CopyToClipboard text={ data.id } isLoading={ isLoading }/>
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel isLoading={ isLoading }>Kernel account</DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue overflow="hidden">
        { data.account === null ? (
          <Text color="text.secondary" data-field="account">—</Text>
        ) : (
          <>
            <Skeleton loading={ isLoading } overflow="hidden" textOverflow="ellipsis" data-field="account">
              { data.account }
            </Skeleton>
            <CopyToClipboard text={ data.account } isLoading={ isLoading }/>
          </>
        ) }
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel isLoading={ isLoading }>Settlement status</DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <StatusLadderBadge rung={ data.status } isLoading={ isLoading }/>
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel isLoading={ isLoading }>Verification status</DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <Skeleton loading={ isLoading } data-field="verification_status">
          { VERIFICATION_STATUS_LABELS[data.verification_status] }
        </Skeleton>
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel isLoading={ isLoading }>Payload hash</DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue overflow="hidden">
        { data.payload_hash === null ? (
          <Text color="text.secondary" data-field="payload_hash">—</Text>
        ) : (
          <>
            <Skeleton loading={ isLoading } overflow="hidden" textOverflow="ellipsis" data-field="payload_hash">
              { data.payload_hash }
            </Skeleton>
            <CopyToClipboard text={ data.payload_hash } isLoading={ isLoading }/>
          </>
        ) }
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel isLoading={ isLoading }>Transaction</DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue overflow="hidden">
        <TxEntity
          hash={ data.transaction_hash }
          isLoading={ isLoading }
          truncation="none"
          noIcon
        />
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel isLoading={ isLoading }>Block</DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <BlockEntity
          number={ data.block_number }
          isLoading={ isLoading }
          truncation="none"
          noIcon
        />
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel isLoading={ isLoading }>Timestamp</DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <TimeWithTooltip
          timestamp={ data.timestamp }
          timeFormat="absolute"
          isLoading={ isLoading }
        />
      </DetailedInfo.ItemValue>
    </DetailedInfo.Container>
  );
};

export default PaxeerXReceiptDetails;
