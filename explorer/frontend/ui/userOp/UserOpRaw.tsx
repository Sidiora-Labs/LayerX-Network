import React from 'react';

import type { UserOp } from 'types/api/userOps';

import { Skeleton } from 'toolkit/chakra/skeleton';
import RawDataSnippet from 'ui/shared/RawDataSnippet';

// order is taken from the ERC-4337 standard
// eslint-disable-next-line max-len
const KEYS_ORDER: Array<keyof UserOp['raw']> = [ 'sender', 'nonce', 'init_code', 'call_data', 'account_gas_limits', 'call_gas_limit', 'verification_gas_limit', 'pre_verification_gas', 'gas_fees', 'max_fee_per_gas', 'max_priority_fee_per_gas', 'paymaster_and_data', 'signature' ];

interface Props {
  rawData?: UserOp['raw'];
  isLoading?: boolean;
}

const UserOpRaw = ({ rawData, isLoading }: Props) => {
  if (!rawData) {
    return null;
  }

  const text = JSON.stringify(KEYS_ORDER.reduce((res: UserOp['raw'], key: keyof UserOp['raw']) => {
    const value = rawData[key];
    if (value !== undefined) {
      res[key] = value;
    }
    return res;
  }, {} as UserOp['raw']), undefined, 4);

  return <Skeleton loading={ isLoading }><RawDataSnippet data={ text }/></Skeleton>;
};

export default UserOpRaw;
