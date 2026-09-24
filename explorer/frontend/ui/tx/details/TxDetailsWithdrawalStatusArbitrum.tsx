import { Text } from '@chakra-ui/react';
import React from 'react';

import type { ArbitrumTransactionMessageStatus, Transaction } from 'types/api/transaction';

import { route } from 'nextjs-routes';

import { layerLabels } from 'lib/rollups/utils';
import { Link } from 'toolkit/chakra/link';
import * as DetailedInfo from 'ui/shared/DetailedInfo/DetailedInfo';
import TxEntityL1 from 'ui/shared/entities/tx/TxEntityL1';
import VerificationSteps from 'ui/shared/verificationSteps/VerificationSteps';

const WITHDRAWAL_STATUS_STEPS: Array<ArbitrumTransactionMessageStatus> = [
  'Syncing with base layer',
  'Settlement pending',
  'Waiting for confirmation',
  'Ready for relay',
  'Relayed',
];

interface Props {
  data: Transaction;
}

const TxDetailsWithdrawalStatusArbitrum = ({ data }: Props) => {

  const steps = React.useMemo(() => {
    if (!data.arbitrum?.message_related_info) {
      return [];
    }

    switch (data.arbitrum.message_related_info.message_status) {
      case 'Ready for relay':
      case 'Relayed': {
        const lastElementIndex = data.arbitrum.message_related_info.message_status === 'Relayed' ? Infinity : -1;

        return WITHDRAWAL_STATUS_STEPS.slice(0, lastElementIndex).map((step, index, array) => {
          if (index !== array.length - 1) {
            return step;
          }

          return {
            content: (
              <Link
                href={ route({ pathname: '/txn-withdrawals', query: { q: data.hash } }) }
              >
                { step }
              </Link>
            ),
            label: step,
          };
        });
      }

      default:
        return WITHDRAWAL_STATUS_STEPS;
    }
  }, [ data.arbitrum?.message_related_info, data.hash ]);

  if (!data.arbitrum || !data.arbitrum?.contains_message || !data.arbitrum?.message_related_info) {
    return null;
  }

  if (data.arbitrum.contains_message === 'outcoming') {
    return (
      <>
        <DetailedInfo.ItemLabel
          hint="Detailed status progress of the transaction"
        >
          Withdrawal status
        </DetailedInfo.ItemLabel>
        <DetailedInfo.ItemValue>
          { data.arbitrum.message_related_info.message_status ? (
            <VerificationSteps
              steps={ steps as unknown as Array<ArbitrumTransactionMessageStatus> }
              currentStep={ data.arbitrum.message_related_info.message_status }
            />
          ) : <Text color="text.secondary">Could not determine</Text> }
        </DetailedInfo.ItemValue>
      </>
    );
  }
  return (
    <>
      <DetailedInfo.ItemLabel
        hint="The hash of the transaction that originated the message from the base layer"
      >
        Originating { layerLabels.parent } txn hash
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        { data.arbitrum.message_related_info.associated_l1_transaction_hash ?
          <TxEntityL1 hash={ data.arbitrum.message_related_info.associated_l1_transaction_hash }/> :
          <Text color="text.secondary">Waiting for confirmation</Text>
        }
      </DetailedInfo.ItemValue>
    </>
  );
};

export default React.memo(TxDetailsWithdrawalStatusArbitrum);
