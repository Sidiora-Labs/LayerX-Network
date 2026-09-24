import { Flex } from '@chakra-ui/react';
import { useCallback, useState } from 'react';

import type { EssentialDappsChainConfig } from 'types/client/marketplace';
import type { AllowanceType } from 'types/client/revoke';

import { Button } from 'toolkit/chakra/button';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { TableRow, TableCell } from 'toolkit/chakra/table';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import TokenEntity from 'ui/shared/entities/token/TokenEntity';
import NumberEntity from 'ui/shared/NumberEntity';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';

import useRevoke from '../hooks/useRevoke';
import formatAllowance from '../lib/formatAllowance';

type Props = {
  selectedChain: EssentialDappsChainConfig | undefined;
  approval: AllowanceType;
  isLoading?: boolean;
  isAddressMatch?: boolean;
  hideApproval: (approval: AllowanceType) => void;
};

export default function ApprovalsTableItem({
  selectedChain,
  approval,
  isLoading,
  isAddressMatch,
  hideApproval,
}: Props) {
  const revoke = useRevoke();
  const [ isPending, setIsPending ] = useState(false);

  const allowance = formatAllowance(approval);

  const handleRevoke = useCallback(async() => {
    setIsPending(true);
    const success = await revoke(approval, Number(selectedChain?.id));
    if (success) {
      hideApproval(approval);
    }
    setIsPending(false);
  }, [ revoke, hideApproval, approval, selectedChain?.id ]);

  return (
    <TableRow fontWeight="500">
      <TableCell verticalAlign="middle">
        <Flex flexDir="column" gap={ 2 } mr={ 2 }>
          <TokenEntity
            token={{
              address_hash: approval.address,
              type: approval.type,
              symbol: approval.symbol || null,
              name: approval.name || null,
              icon_url: approval.tokenIcon || null,
              reputation: approval.tokenReputation,
            }}
            isLoading={ isLoading }
            noCopy
            jointSymbol
            textStyle="sm"
            fontWeight="600"
            chain={ selectedChain }
            link={{ noIcon: true, external: true }}
          />
          <AddressEntity
            address={{ hash: approval.address }}
            truncation="constant"
            noIcon
            isLoading={ isLoading }
            chain={ selectedChain }
            link={{ variant: 'secondary', noIcon: true, external: true }}
          />
        </Flex>
      </TableCell>
      <TableCell verticalAlign="middle">
        <AddressEntity
          address={{ hash: approval.spender }}
          truncation="constant"
          noIcon
          isLoading={ isLoading }
          chain={ selectedChain }
          link={{ noIcon: true, external: true }}
        />
      </TableCell>
      <TableCell isNumeric verticalAlign="middle">
        <Skeleton loading={ isLoading } display="inline-block">
          <NumberEntity
            value={ allowance }
            postfix={
              [ 'Unlimited', 'N/A' ].includes(allowance) ? '' : approval.symbol
            }
          />
        </Skeleton>
      </TableCell>
      <TableCell isNumeric verticalAlign="middle">
        <Skeleton loading={ isLoading } display="inline-block">
          { approval.valueAtRiskUsd && (
            <NumberEntity
              value={ approval.valueAtRiskUsd.toString() }
              suffix="$"
            />
          ) }
        </Skeleton>
      </TableCell>
      <TableCell></TableCell>
      <TableCell verticalAlign="middle">
        <TimeWithTooltip timestamp={ approval.timestamp } isLoading={ isLoading }/>
      </TableCell>
      { isAddressMatch && (
        <TableCell isNumeric verticalAlign="middle">
          <Button
            size="sm"
            variant="outline"
            loading={ isLoading || isPending }
            onClick={ handleRevoke }
          >
            Revoke
          </Button>
        </TableCell>
      ) }
    </TableRow>
  );
}
