import { Flex } from '@chakra-ui/react';
import React from 'react';

import type { ValidatorZilliqa } from 'types/api/validators';

import { Skeleton } from 'toolkit/chakra/skeleton';
import CopyToClipboard from 'ui/shared/CopyToClipboard';
import * as DetailedInfo from 'ui/shared/DetailedInfo/DetailedInfo';
import DetailedInfoSponsoredItem from 'ui/shared/DetailedInfo/DetailedInfoSponsoredItem';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import BlockEntity from 'ui/shared/entities/block/BlockEntity';
import HashStringShortenDynamic from 'ui/shared/HashStringShortenDynamic';
import NativeTokenIcon from 'ui/shared/NativeTokenIcon';
import NativeCoinValue from 'ui/shared/value/NativeCoinValue';

interface Props {
  data: ValidatorZilliqa;
  isLoading: boolean;
}

const ValidatorDetails = ({ data, isLoading }: Props) => {
  return (
    <DetailedInfo.Container>
      <DetailedInfo.ItemLabel
        hint="Index of the staker in the committee"
        isLoading={ isLoading }
      >
        Index
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <Skeleton loading={ isLoading } display="inline">
          { data.index }
        </Skeleton>
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel
        hint="Staker's balance"
        isLoading={ isLoading }
      >
        Staked
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <NativeCoinValue
          startElement={ <NativeTokenIcon isLoading={ isLoading } boxSize={ 5 } mr={ 2 }/> }
          amount={ data.balance }
          loading={ isLoading }
        />
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel
        hint="libp2p peer ID, corresponding to the staker's BLS public key"
        isLoading={ isLoading }
      >
        Peer ID
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <Flex alignItems="center" w="100%" minWidth={ 0 }>
          <Skeleton loading={ isLoading } maxW="calc(100% - 28px)" overflow="hidden">
            <HashStringShortenDynamic hash={ data.peer_id }/>
          </Skeleton>
          <CopyToClipboard text={ data.peer_id } isLoading={ isLoading }/>
        </Flex>
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel
        hint="The address used for authenticating requests from this staker to the deposit contract"
        isLoading={ isLoading }
      >
        Control address
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <AddressEntity address={ data.control_address } isLoading={ isLoading }/>
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel
        hint="The address which rewards for this staker will be sent to"
        isLoading={ isLoading }
      >
        Reward address
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <AddressEntity address={ data.reward_address } isLoading={ isLoading }/>
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel
        hint="The address whose key the validator uses to sign cross-chain events"
        isLoading={ isLoading }
      >
        Signing address
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <AddressEntity address={ data.signing_address } isLoading={ isLoading }/>
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel
        hint="Block number at which the staker was added"
        isLoading={ isLoading }
      >
        Added at block
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <BlockEntity number={ data.added_at_block_number } isLoading={ isLoading }/>
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel
        hint="Block number at which the staker's stake was last updated"
        isLoading={ isLoading }
      >
        Stake updated
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <BlockEntity number={ data.stake_updated_at_block_number } isLoading={ isLoading }/>
      </DetailedInfo.ItemValue>

      <DetailedInfoSponsoredItem isLoading={ isLoading }/>
    </DetailedInfo.Container>
  );
};

export default React.memo(ValidatorDetails);
