import { upperFirst } from 'es-toolkit';
import React from 'react';

import type { Address } from 'types/api/address';
import type { ExcludeNull, ExcludeUndefined } from 'types/utils';

import { currencyUnits } from 'lib/units';
import { Link } from 'toolkit/chakra/link';
import { TruncatedText } from 'toolkit/components/truncation/TruncatedText';
import * as DetailedInfo from 'ui/shared/DetailedInfo/DetailedInfo';
import * as DetailedInfoItemBreakdown from 'ui/shared/DetailedInfo/DetailedInfoItemBreakdown';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import NativeCoinValue from 'ui/shared/value/NativeCoinValue';

interface Props {
  isLoading?: boolean;
  data: ExcludeNull<ExcludeUndefined<Address['celo']>['account']>;
}

const AddressCeloAccount = ({ isLoading, data }: Props) => {
  return (
    <>
      <DetailedInfo.ItemLabel
        hint="Celo account info (from the Accounts contract), including it's human-readable name"
        isLoading={ isLoading }
      >
        Celo account
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue multiRow>
        { data.name && <TruncatedText text={ data.name } mr={ 3 }/> }
        <DetailedInfoItemBreakdown.Container loading={ isLoading }>
          <DetailedInfoItemBreakdown.Row
            label="Type"
            hint="The role of the account: regular, validator, or validator group"
          >
            { upperFirst(data.type) }
          </DetailedInfoItemBreakdown.Row>

          { data.metadata_url && (
            <DetailedInfoItemBreakdown.Row
              label="Metadata URL"
              hint="Link to additional information published by the account owner"
            >
              <Link href={ data.metadata_url } external>
                <TruncatedText text={ data.metadata_url }/>
              </Link>
            </DetailedInfoItemBreakdown.Row>
          ) }

          <DetailedInfoItemBreakdown.Row
            label={ `Locked ${ currencyUnits.ether }` }
            hint="Total amount of CELO locked by this account (used for staking or governance)"
          >
            <NativeCoinValue amount={ data.locked_celo } noSymbol/>
          </DetailedInfoItemBreakdown.Row>

          <DetailedInfoItemBreakdown.Row
            label={ `Non-voting locked ${ currencyUnits.ether }` }
            hint="Portion of locked CELO that is not currently used for voting"
          >
            <NativeCoinValue amount={ data.nonvoting_locked_celo } noSymbol/>
          </DetailedInfoItemBreakdown.Row>

          { data.vote_signer_address && (
            <DetailedInfoItemBreakdown.Row
              label="Vote signer address"
              hint="Address authorized to vote in governance and validator elections on behalf of this account"
            >
              <AddressEntity address={ data.vote_signer_address }/>
            </DetailedInfoItemBreakdown.Row>
          ) }

          { data.validator_signer_address && (
            <DetailedInfoItemBreakdown.Row
              label="Validator signer address"
              hint="Address authorized to manage a validator or validator group and sign consensus messages for this account"
            >
              <AddressEntity address={ data.validator_signer_address }/>
            </DetailedInfoItemBreakdown.Row>
          ) }

          { data.attestation_signer_address && (
            <DetailedInfoItemBreakdown.Row
              label="Attestation signer address"
              hint="Address whose key this account uses to sign attestations on the Attestations contract"
            >
              <AddressEntity address={ data.attestation_signer_address }/>
            </DetailedInfoItemBreakdown.Row>
          ) }
        </DetailedInfoItemBreakdown.Container>
      </DetailedInfo.ItemValue>
    </>
  );
};

export default React.memo(AddressCeloAccount);
