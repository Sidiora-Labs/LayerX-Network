import React from 'react';

import type { UserOpsItem } from 'types/api/userOps';
import type { ClusterChainConfig } from 'types/multichain';

import config from 'configs/app';
import { useMultichainContext } from 'lib/contexts/multichain';
import AddressStringOrParam from 'ui/shared/entities/address/AddressStringOrParam';
import BlockEntity from 'ui/shared/entities/block/BlockEntity';
import TxEntity from 'ui/shared/entities/tx/TxEntity';
import UserOpEntity from 'ui/shared/entities/userOp/UserOpEntity';
import ListItemMobileGrid from 'ui/shared/ListItemMobile/ListItemMobileGrid';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';
import UserOpStatus from 'ui/shared/userOps/UserOpStatus';
import NativeCoinValue from 'ui/shared/value/NativeCoinValue';

type Props = {
  item: UserOpsItem;
  isLoading?: boolean;
  showTx: boolean;
  showSender: boolean;
  chainData?: ClusterChainConfig;
};

const UserOpsListItem = ({ item, isLoading, showTx, showSender, chainData }: Props) => {
  const multichainContext = useMultichainContext();
  const chainConfig = (multichainContext?.chain.app_config || config);

  return (
    <ListItemMobileGrid.Container gridTemplateColumns="100px auto">

      <ListItemMobileGrid.Label isLoading={ isLoading }>User op hash</ListItemMobileGrid.Label>
      <ListItemMobileGrid.Value>
        <UserOpEntity hash={ item.hash } isLoading={ isLoading } fontWeight="700" noIcon={ !chainData } truncation="constant_long" chain={ chainData } noCopy/>
      </ListItemMobileGrid.Value>

      <ListItemMobileGrid.Label isLoading={ isLoading }>Age</ListItemMobileGrid.Label>
      <ListItemMobileGrid.Value>
        <TimeWithTooltip
          timestamp={ item.timestamp }
          isLoading={ isLoading }
          color="text.secondary"
          display="inline-block"
        />
      </ListItemMobileGrid.Value>

      <ListItemMobileGrid.Label isLoading={ isLoading }>Status</ListItemMobileGrid.Label>
      <ListItemMobileGrid.Value>
        <UserOpStatus status={ item.status } isLoading={ isLoading }/>
      </ListItemMobileGrid.Value>

      { showSender && (
        <>
          <ListItemMobileGrid.Label isLoading={ isLoading }>Sender</ListItemMobileGrid.Label>
          <ListItemMobileGrid.Value>
            <AddressStringOrParam
              address={ item.address }
              isLoading={ isLoading }
              truncation="constant"
            />
          </ListItemMobileGrid.Value>
        </>
      ) }

      { showTx && (
        <>
          <ListItemMobileGrid.Label isLoading={ isLoading }>Tx hash</ListItemMobileGrid.Label>
          <ListItemMobileGrid.Value>
            <TxEntity
              hash={ item.transaction_hash }
              isLoading={ isLoading }
              noIcon
              truncation="constant_long"
            />
          </ListItemMobileGrid.Value>
        </>
      ) }

      <ListItemMobileGrid.Label isLoading={ isLoading }>Block</ListItemMobileGrid.Label>
      <ListItemMobileGrid.Value>
        <BlockEntity
          number={ Number(item.block_number) }
          isLoading={ isLoading }
          textStyle="sm"
          noIcon
        />
      </ListItemMobileGrid.Value>

      { !chainConfig.UI.views.tx.hiddenFields?.tx_fee && (
        <>
          <ListItemMobileGrid.Label isLoading={ isLoading }>Fee</ListItemMobileGrid.Label>
          <ListItemMobileGrid.Value>
            <NativeCoinValue
              amount={ item.fee }
              loading={ isLoading }
            />
          </ListItemMobileGrid.Value>
        </>
      ) }

    </ListItemMobileGrid.Container>
  );
};

export default UserOpsListItem;
