import React from 'react';

import type { EssentialDappsChainConfig } from 'types/client/marketplace';
import type { AllowanceType } from 'types/client/revoke';

import { TableBody, TableColumnHeader, TableHeaderSticky, TableRoot, TableRow } from 'toolkit/chakra/table';
import TimeFormatToggle from 'ui/shared/time/TimeFormatToggle';

import ApprovalsTableItem from './ApprovalsTableItem';

type Props = {
  selectedChain: EssentialDappsChainConfig | undefined;
  approvals: Array<AllowanceType>;
  isLoading?: boolean;
  isAddressMatch?: boolean;
  hideApproval: (approval: AllowanceType) => void;
};

export default function ApprovalsTable({
  selectedChain,
  approvals,
  isLoading,
  isAddressMatch,
  hideApproval,
}: Props) {
  return (
    <TableRoot>
      <TableHeaderSticky top={ 136 }>
        <TableRow>
          <TableColumnHeader w="30%">Token</TableColumnHeader>
          <TableColumnHeader w="15%">Approved spender</TableColumnHeader>
          <TableColumnHeader w="20%" isNumeric>
            Approved amount
          </TableColumnHeader>
          <TableColumnHeader w="17%" isNumeric>
            Value at risk
          </TableColumnHeader>
          <TableColumnHeader w={ isAddressMatch ? '30px' : '50px' }/>
          <TableColumnHeader w="18%">
            Last updated
            <TimeFormatToggle/>
          </TableColumnHeader>
          { isAddressMatch && <TableColumnHeader w="95px" isNumeric/> }
        </TableRow>
      </TableHeaderSticky>
      <TableBody>
        { approvals.map((approval, index) => (
          <ApprovalsTableItem
            key={ index }
            selectedChain={ selectedChain }
            approval={ approval }
            isLoading={ isLoading }
            isAddressMatch={ isAddressMatch }
            hideApproval={ hideApproval }
          />
        )) }
      </TableBody>
    </TableRoot>
  );
}
