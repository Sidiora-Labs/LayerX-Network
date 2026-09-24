import { GridItem } from '@chakra-ui/react';
import type { UseQueryResult } from '@tanstack/react-query';
import BigNumber from 'bignumber.js';
import React from 'react';

import type { UserOp } from 'types/api/userOps';

import config from 'configs/app';
import type { ResourceError } from 'lib/api/resources';
import throwOnResourceLoadError from 'lib/errors/throwOnResourceLoadError';
import { CollapsibleDetails } from 'toolkit/chakra/collapsible';
import { Skeleton } from 'toolkit/chakra/skeleton';
import isCustomAppError from 'ui/shared/AppError/isCustomAppError';
import DataFetchAlert from 'ui/shared/DataFetchAlert';
import * as DetailedInfo from 'ui/shared/DetailedInfo/DetailedInfo';
import DetailedInfoNativeCoinValue from 'ui/shared/DetailedInfo/DetailedInfoNativeCoinValue';
import DetailedInfoTimestamp from 'ui/shared/DetailedInfo/DetailedInfoTimestamp';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import AddressStringOrParam from 'ui/shared/entities/address/AddressStringOrParam';
import BlockEntity from 'ui/shared/entities/block/BlockEntity';
import TxEntity from 'ui/shared/entities/tx/TxEntity';
import UserOpEntity from 'ui/shared/entities/userOp/UserOpEntity';
import UserOpSponsorType from 'ui/shared/userOps/UserOpSponsorType';
import UserOpStatus from 'ui/shared/userOps/UserOpStatus';
import Utilization from 'ui/shared/Utilization/Utilization';
import GasPriceValue from 'ui/shared/value/GasPriceValue';

import UserOpCallData from './UserOpCallData';
import UserOpDecodedCallData from './UserOpDecodedCallData';
import UserOpDetailsActions from './UserOpDetailsActions';

interface Props {
  query: UseQueryResult<UserOp, ResourceError>;
}

const UserOpDetails = ({ query }: Props) => {
  const { data, isPlaceholderData, isError, error } = query;

  if (isError) {
    if (error?.status === 400 || isCustomAppError(error)) {
      throwOnResourceLoadError({ isError, error });
    }

    return <DataFetchAlert/>;
  }

  if (!data) {
    return null;
  }

  return (
    <DetailedInfo.Container
      templateColumns={{ base: 'minmax(0, 1fr)', lg: 'minmax(min-content, 220px) minmax(0, 1fr)' }}
    >
      <DetailedInfo.ItemLabel
        hint="Unique character string assigned to every User operation"
        isLoading={ isPlaceholderData }
      >
        User operation hash
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <Skeleton loading={ isPlaceholderData } overflow="hidden">
          <UserOpEntity hash={ data.hash } noIcon noLink/>
        </Skeleton>
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel
        hint="The address of the smart contract account"
        isLoading={ isPlaceholderData }
      >
        Sender
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <AddressStringOrParam address={ data.sender } isLoading={ isPlaceholderData }/>
      </DetailedInfo.ItemValue>

      { data.execute_target && (
        <>
          <DetailedInfo.ItemLabel
            hint="Target smart contract called by the User operation"
            isLoading={ isPlaceholderData }
          >
            Target
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue>
            <AddressEntity address={ data.execute_target } isLoading={ isPlaceholderData }/>
          </DetailedInfo.ItemValue>
        </>
      ) }

      <DetailedInfo.ItemLabel
        hint="Current User operation state"
        isLoading={ isPlaceholderData }
      >
        Status
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <UserOpStatus status={ data.status } isLoading={ isPlaceholderData }/>
      </DetailedInfo.ItemValue>

      { data.revert_reason && (
        <>
          <DetailedInfo.ItemLabel
            hint="The revert reason of the User operation"
            isLoading={ isPlaceholderData }
          >
            Revert reason
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue
            wordBreak="break-all"
            whiteSpace="normal"
          >
            <Skeleton loading={ isPlaceholderData }>
              { data.revert_reason }
            </Skeleton>
          </DetailedInfo.ItemValue>
        </>
      ) }

      { data.timestamp && (
        <>
          <DetailedInfo.ItemLabel
            hint="Date and time of User operation"
            isLoading={ isPlaceholderData }
          >
            Timestamp
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue>
            <DetailedInfoTimestamp timestamp={ data.timestamp } isLoading={ isPlaceholderData }/>
          </DetailedInfo.ItemValue>
        </>

      ) }
      { !config.UI.views.tx.hiddenFields?.tx_fee && (
        <>
          <DetailedInfo.ItemLabel
            hint="Total User operation fee"
            isLoading={ isPlaceholderData }
          >
            Fee
          </DetailedInfo.ItemLabel>
          <DetailedInfoNativeCoinValue
            amount={ data.fee }
            loading={ isPlaceholderData }
          />
        </>
      ) }

      <DetailedInfo.ItemLabel
        hint="Gas limit for the User operation"
        isLoading={ isPlaceholderData }
      >
        Gas limit
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <Skeleton loading={ isPlaceholderData }>
          { BigNumber(data.gas).toFormat() }
        </Skeleton>
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel
        hint="Actual gas amount used by the User operation"
        isLoading={ isPlaceholderData }
      >
        Gas used
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <Skeleton loading={ isPlaceholderData }>
          { BigNumber(data.gas_used).toFormat() }
        </Skeleton>
        <Utilization
          ml={ 4 }
          colorScheme="gray"
          value={ BigNumber(data.gas_used).dividedBy(BigNumber(data.gas)).toNumber() }
          isLoading={ isPlaceholderData }
        />
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel
        hint="Hash of the transaction this User operation belongs to"
        isLoading={ isPlaceholderData }
      >
        Transaction hash
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <TxEntity hash={ data.transaction_hash } isLoading={ isPlaceholderData }/>
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel
        hint="Block number containing this User operation"
        isLoading={ isPlaceholderData }
      >
        Block
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <BlockEntity number={ Number(data.block_number) } isLoading={ isPlaceholderData }/>
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel
        hint="Contract that executes bundles of User operations"
        isLoading={ isPlaceholderData }
      >
        Entry point
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <AddressStringOrParam address={ data.entry_point } isLoading={ isPlaceholderData }/>
      </DetailedInfo.ItemValue>

      { config.features.txInterpretation.isEnabled && <UserOpDetailsActions hash={ data.hash } isUserOpDataLoading={ isPlaceholderData }/> }

      { /* ADDITIONAL INFO */ }
      <CollapsibleDetails loading={ isPlaceholderData } mt={ 6 } gridColumn={{ base: undefined, lg: '1 / 3' }}>
        <GridItem colSpan={{ base: undefined, lg: 2 }} mt={{ base: 1, lg: 4 }}/>

        <DetailedInfo.ItemLabel
          hint="Gas limit for execution phase"
        >
          Call gas limit
        </DetailedInfo.ItemLabel>
        <DetailedInfo.ItemValue>
          { BigNumber(data.call_gas_limit).toFormat() }
        </DetailedInfo.ItemValue>

        <DetailedInfo.ItemLabel
          hint="Gas limit for verification phase"
        >
          Verification gas limit
        </DetailedInfo.ItemLabel>
        <DetailedInfo.ItemValue>
          { BigNumber(data.verification_gas_limit).toFormat() }
        </DetailedInfo.ItemValue>

        <DetailedInfo.ItemLabel
          hint="Gas to compensate the bundler"
        >
          Pre-verification gas
        </DetailedInfo.ItemLabel>
        <DetailedInfo.ItemValue>
          { BigNumber(data.pre_verification_gas).toFormat() }
        </DetailedInfo.ItemValue>

        { !config.UI.views.tx.hiddenFields?.gas_fees && (
          <>
            <DetailedInfo.ItemLabel
              hint="Maximum fee per gas "
            >
              Max fee per gas
            </DetailedInfo.ItemLabel>
            <DetailedInfo.ItemValue multiRow>
              <GasPriceValue amount={ data.max_fee_per_gas }/>
            </DetailedInfo.ItemValue>

            <DetailedInfo.ItemLabel
              hint="Maximum priority fee per gas"
            >
              Max priority fee per gas
            </DetailedInfo.ItemLabel>
            <DetailedInfo.ItemValue multiRow>
              <GasPriceValue amount={ data.max_priority_fee_per_gas }/>
            </DetailedInfo.ItemValue>
          </>
        ) }

        <DetailedInfo.ItemDivider/>

        { data.aggregator && (
          <>
            <DetailedInfo.ItemLabel
              hint="Helper contract to validate an aggregated signature"
            >
              Aggregator
            </DetailedInfo.ItemLabel>
            <DetailedInfo.ItemValue>
              <AddressStringOrParam address={ data.aggregator }/>
            </DetailedInfo.ItemValue>
          </>
        ) }

        { data.aggregator_signature && (
          <>
            <DetailedInfo.ItemLabel
              hint="Aggregator signature"
            >
              Aggregator signature
            </DetailedInfo.ItemLabel>
            <DetailedInfo.ItemValue>
              { data.aggregator_signature }
            </DetailedInfo.ItemValue>
          </>
        ) }

        <DetailedInfo.ItemLabel
          hint="A node (block builder) that handles User operations"
        >
          Bundler
        </DetailedInfo.ItemLabel>
        <DetailedInfo.ItemValue>
          <AddressStringOrParam address={ data.bundler }/>
        </DetailedInfo.ItemValue>

        { data.factory && (
          <>
            <DetailedInfo.ItemLabel
              hint="Smart contract that deploys new smart contract wallets for users"
            >
              Factory
            </DetailedInfo.ItemLabel>
            <DetailedInfo.ItemValue>
              <AddressStringOrParam address={ data.factory }/>
            </DetailedInfo.ItemValue>
          </>
        ) }

        { data.paymaster && (
          <>
            <DetailedInfo.ItemLabel
              hint="Contract to sponsor the gas fees for User operations"
            >
              Paymaster
            </DetailedInfo.ItemLabel>
            <DetailedInfo.ItemValue>
              <AddressStringOrParam address={ data.paymaster }/>
            </DetailedInfo.ItemValue>
          </>
        ) }

        <DetailedInfo.ItemLabel
          hint="Type of the gas fees sponsor"
        >
          Sponsor type
        </DetailedInfo.ItemLabel>
        <DetailedInfo.ItemValue>
          <UserOpSponsorType sponsorType={ data.sponsor_type }/>
        </DetailedInfo.ItemValue>

        <DetailedInfo.ItemDivider/>

        <DetailedInfo.ItemLabel
          hint="Used to validate a User operation along with the nonce during verification"
        >
          Signature
        </DetailedInfo.ItemLabel>
        <DetailedInfo.ItemValue
          wordBreak="break-all"
          whiteSpace="normal"
        >
          { data.signature }
        </DetailedInfo.ItemValue>

        <DetailedInfo.ItemLabel
          hint="Anti-replay protection; also used as the salt for first-time account creation"
        >
          Nonce
        </DetailedInfo.ItemLabel>
        <DetailedInfo.ItemValue
          wordBreak="break-all"
          whiteSpace="normal"
        >
          { data.nonce }
        </DetailedInfo.ItemValue>

        <UserOpCallData data={ data }/>

        <UserOpDecodedCallData data={ data }/>
      </CollapsibleDetails>
    </DetailedInfo.Container>
  );
};

export default UserOpDetails;
