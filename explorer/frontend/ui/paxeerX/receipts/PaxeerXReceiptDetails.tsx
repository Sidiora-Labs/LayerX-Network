import { Box, Text } from '@chakra-ui/react';
import React from 'react';

import type { PaxeerXReceipt, PaxeerXVerificationStatus } from 'types/api/paxeerXLists';

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
}

const PaxeerXReceiptDetails = ({ data }: Props) => {
  return (
    <DetailedInfo.Container>
      <DetailedInfo.ItemLabel>Receipt ID</DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue overflow="hidden">
        <Box overflow="hidden" textOverflow="ellipsis" data-field="id">{ data.id }</Box>
        <CopyToClipboard text={ data.id }/>
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel>Kernel account</DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue overflow="hidden">
        { data.account === null ? (
          <Text color="text.secondary" data-field="account">—</Text>
        ) : (
          <>
            <Box overflow="hidden" textOverflow="ellipsis" data-field="account">{ data.account }</Box>
            <CopyToClipboard text={ data.account }/>
          </>
        ) }
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel>Settlement status</DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <StatusLadderBadge rung={ data.status }/>
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel>Verification status</DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <Box data-field="verification_status">{ VERIFICATION_STATUS_LABELS[data.verification_status] }</Box>
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel>Payload hash</DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue overflow="hidden">
        { data.payload_hash === null ? (
          <Text color="text.secondary" data-field="payload_hash">—</Text>
        ) : (
          <>
            <Box overflow="hidden" textOverflow="ellipsis" data-field="payload_hash">{ data.payload_hash }</Box>
            <CopyToClipboard text={ data.payload_hash }/>
          </>
        ) }
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel>Transaction</DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue overflow="hidden">
        <TxEntity
          hash={ data.transaction_hash }
          truncation="none"
          noIcon
        />
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel>Block</DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <BlockEntity
          number={ data.block_number }
          truncation="none"
          noIcon
        />
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel>Timestamp</DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <TimeWithTooltip
          timestamp={ data.timestamp }
          timeFormat="absolute"
        />
      </DetailedInfo.ItemValue>
    </DetailedInfo.Container>
  );
};

export default PaxeerXReceiptDetails;
