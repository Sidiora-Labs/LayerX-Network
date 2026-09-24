import React from 'react';

import type { AddressParam } from 'types/api/addressParams';
import type { OptimisticL2WithdrawalClaimInfo } from 'types/api/optimisticL2';

import config from 'configs/app';
import { parentChain } from 'lib/web3/chains';
import { Button } from 'toolkit/chakra/button';
import { Link } from 'toolkit/chakra/link';
import { useDisclosure } from 'toolkit/hooks/useDisclosure';

import OptimisticL2ClaimModal from './OptimisticL2ClaimModal';

const rollupFeature = config.features.rollup;

export const canClaimDirectlyGuard = (data: OptimisticL2WithdrawalClaimInfo) => {
  return (
    config.features.blockchainInteraction.isEnabled &&
    Boolean(parentChain) &&
    data.portal_contract_address_hash !== null &&
    data.msg_sender_address_hash !== null &&
    data.msg_target_address_hash !== null &&
    data.msg_data !== null &&
    data.msg_gas_limit !== null &&
    data.msg_nonce_raw !== null &&
    data.msg_value !== null
  );
};

interface Props {
  data: OptimisticL2WithdrawalClaimInfo;
  from: AddressParam | null;
  onSuccess: (txHash: string) => void;
  source: 'list' | 'tx';
}

const OptimisticL2ClaimButton = ({ data, from, onSuccess, source }: Props) => {

  const modal = useDisclosure();

  if (canClaimDirectlyGuard(data)) {
    return (
      <>
        { modal.open && (
          <OptimisticL2ClaimModal
            data={ data }
            onOpenChange={ modal.onOpenChange }
            proofSubmitterAddress={ from?.hash }
            onSuccess={ onSuccess }
          />
        ) }
        <Button variant="outline" size="sm" onClick={ modal.onOpen }>Claim</Button>
      </>
    );
  }

  if (!rollupFeature.isEnabled || !rollupFeature.L2WithdrawalUrl) {
    if (source === 'list') {
      return 'Ready for relay';
    }
    return null;
  }

  if (source === 'list') {
    return (
      <Link
        href={ rollupFeature.L2WithdrawalUrl }
        external
      >
        Ready for relay
      </Link>
    );
  }

  return (
    <Link
      href={ rollupFeature.L2WithdrawalUrl }
      external
      noIcon
    >
      <Button variant="outline" size="sm">Claim</Button>
    </Link>
  );
};

export default React.memo(OptimisticL2ClaimButton);
