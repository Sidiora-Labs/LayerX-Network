import { useQueryClient } from '@tanstack/react-query';
import React from 'react';

import type { AddressParam } from 'types/api/addressParams';
import type { OptimisticL2WithdrawalStatus } from 'types/api/optimisticL2';
import type { OpWithdrawal, Transaction } from 'types/api/transaction';

import config from 'configs/app';
import { getResourceKey } from 'lib/api/useApiQuery';
import TxEntityL1 from 'ui/shared/entities/tx/TxEntityL1';
import VerificationSteps from 'ui/shared/verificationSteps/VerificationSteps';
import OptimisticL2ClaimButton, { canClaimDirectlyGuard } from 'ui/withdrawals/optimisticL2/OptimisticL2ClaimButton';

interface Props {
  data: OpWithdrawal;
  from: AddressParam;
  txHash: string;
}

const WITHDRAWAL_STATUS_STEPS: Array<OptimisticL2WithdrawalStatus> = [
  'Waiting for state root',
  'Ready to prove',
  'In challenge period',
  'Ready for relay',
  'Relayed',
];

const WITHDRAWAL_STATUS_ORDER_PROVEN: Array<OptimisticL2WithdrawalStatus> = [
  'Waiting for state root',
  'Ready to prove',
  'Proven',
  'Relayed',
];

const WITHDRAWAL_STATUS_ORDER_GAME: Array<OptimisticL2WithdrawalStatus> = [
  'Waiting for state root',
  'Ready to prove',
  'Waiting a game to resolve',
  'In challenge period',
  'Ready for relay',
  'Relayed',
];

const rollupFeature = config.features.rollup;

const TxDetailsWithdrawalStatusOptimistic = ({ data, from, txHash }: Props) => {
  const queryClient = useQueryClient();

  const handleClaimSuccess = React.useCallback((l1TxHash: string) => {
    queryClient.setQueryData(
      getResourceKey('general:tx', { pathParams: { hash: txHash } }),
      (prevData: Transaction | undefined) => {
        if (!prevData) {
          return;
        }

        const newWithdrawals = prevData.op_withdrawals?.map((withdrawal) => {
          if (withdrawal.nonce === data.nonce) {
            return {
              ...withdrawal,
              l1_transaction_hash: l1TxHash,
              status: 'Relayed',
            };
          }
          return withdrawal;
        });

        return {
          ...prevData,
          op_withdrawals: newWithdrawals,
        };
      });
  }, [ data.nonce, queryClient, txHash ]);

  if (!data.status || !rollupFeature.isEnabled || rollupFeature.type !== 'optimistic') {
    return null;
  }

  const canClaimDirectly = canClaimDirectlyGuard(data);
  const hasRightSlot = data.status === 'Ready for relay' && (rollupFeature.L2WithdrawalUrl || canClaimDirectly);

  const rightSlot = hasRightSlot ?
    <OptimisticL2ClaimButton data={ data } from={ from } onSuccess={ handleClaimSuccess } source="tx"/> :
    null;

  const steps = (() => {
    switch (data.status) {
      case 'Ready for relay':
        return hasRightSlot ? WITHDRAWAL_STATUS_STEPS.slice(0, -1) : WITHDRAWAL_STATUS_STEPS;
      case 'Proven':
        return WITHDRAWAL_STATUS_ORDER_PROVEN;
      case 'Waiting a game to resolve':
        return WITHDRAWAL_STATUS_ORDER_GAME;
      case 'Relayed': {
        if (data.l1_transaction_hash) {
          return WITHDRAWAL_STATUS_STEPS.map((status) => {
            return status === 'Relayed' ? {
              content: <TxEntityL1 hash={ data.l1_transaction_hash } truncation="constant" text="Relayed" noIcon noCopy/>,
              label: status,
            } : status;
          });
        }

        return WITHDRAWAL_STATUS_STEPS;
      }

      default:
        return WITHDRAWAL_STATUS_STEPS;
    }
  })();

  return (
    <VerificationSteps
      steps={ steps as unknown as Array<OptimisticL2WithdrawalStatus> }
      currentStep={ data.status }
      rightSlot={ rightSlot }
    />
  );
};

export default React.memo(TxDetailsWithdrawalStatusOptimistic);
