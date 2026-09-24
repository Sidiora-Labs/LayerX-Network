import dynamic from 'next/dynamic';
import React from 'react';

import type { AddressVerificationFormFirstStepFields, AddressCheckStatusSuccess } from './types';
import type { VerifiedAddress } from 'types/api/account';

import config from 'configs/app';
import { FallbackProvider } from 'lib/contexts/fallback';
import * as mixpanel from 'lib/mixpanel/index';
import { DialogBody, DialogContent, DialogHeader, DialogRoot } from 'toolkit/chakra/dialog';

const Web3ProviderBase = dynamic(() => import('ui/shared/web3/Web3Provider'), { ssr: false });

import AddressVerificationStepAddress from './steps/AddressVerificationStepAddress';
import AddressVerificationStepSignature from './steps/AddressVerificationStepSignature';
import AddressVerificationStepSuccess from './steps/AddressVerificationStepSuccess';

type StateData = AddressVerificationFormFirstStepFields & AddressCheckStatusSuccess & { isToken?: boolean };

const feature = config.features.blockchainInteraction;
// Dynamic providers cannot be nested, so a dummy provider is used here
const Web3Provider = feature.isEnabled && feature.connectorType === 'dynamic' ? FallbackProvider : Web3ProviderBase;

interface Props {
  open: boolean;
  onOpenChange: ({ open }: { open: boolean }) => void;
  onSubmit: (address: VerifiedAddress) => void;
  onAddTokenInfoClick: (address: string) => void;
  onShowListClick: () => void;
  defaultAddress?: string;
  pageType: string;
}

const AddressVerificationModal = ({ defaultAddress, open, onOpenChange, onSubmit, onAddTokenInfoClick, onShowListClick, pageType }: Props) => {
  const [ stepIndex, setStepIndex ] = React.useState(0);
  const [ data, setData ] = React.useState<StateData>({ address: '', signingMessage: '' });

  React.useEffect(() => {
    open && mixpanel.logEvent(
      mixpanel.EventTypes.VERIFY_ADDRESS,
      { Action: 'Form opened', 'Page type': pageType },
    );
  }, [ open, pageType ]);

  const handleGoToSecondStep = React.useCallback((firstStepResult: typeof data) => {
    setData(firstStepResult);
    setStepIndex((prev) => prev + 1);
    mixpanel.logEvent(
      mixpanel.EventTypes.VERIFY_ADDRESS,
      { Action: 'Address entered', 'Page type': pageType },
    );
  }, [ pageType ]);

  const handleGoToThirdStep = React.useCallback((address: VerifiedAddress, signMethod: 'wallet' | 'manual') => {
    onSubmit(address);
    setStepIndex((prev) => prev + 1);
    setData((prev) => ({ ...prev, isToken: Boolean(address.metadata.tokenName) }));
    mixpanel.logEvent(
      mixpanel.EventTypes.VERIFY_ADDRESS,
      { Action: 'Sign ownership', 'Page type': pageType, 'Sign method': signMethod },
    );
  }, [ onSubmit, pageType ]);

  const handleGoToPrevStep = React.useCallback(() => {
    setStepIndex((prev) => prev - 1);
  }, []);

  const handleOpenChange = React.useCallback(({ open }: { open: boolean }) => {
    onOpenChange({ open });
    if (!open) {
      setStepIndex(0);
      setData({ address: '', signingMessage: '' });
    }
  }, [ onOpenChange ]);

  const handleAddTokenInfoClick = React.useCallback(() => {
    onAddTokenInfoClick(data.address);
    handleOpenChange({ open: false });
  }, [ handleOpenChange, data.address, onAddTokenInfoClick ]);

  const steps = [
    {
      title: 'Verify new address ownership',
      content: <AddressVerificationStepAddress onContinue={ handleGoToSecondStep } defaultAddress={ defaultAddress }/>,
    },
    {
      title: 'Copy and sign message',
      content: (
        <AddressVerificationStepSignature
          { ...data }
          onContinue={ handleGoToThirdStep }
          noWeb3Provider={ !config.features.blockchainInteraction.isEnabled }
        />
      ),
    },
    {
      title: 'Congrats! Address is verified.',
      content: (
        <AddressVerificationStepSuccess
          onShowListClick={ onShowListClick }
          onAddTokenInfoClick={ handleAddTokenInfoClick }
          isToken={ data.isToken }
          address={ data.address }
        />
      ),
    },
  ];
  const step = steps[stepIndex];

  return (
    <DialogRoot
      open={ open }
      onOpenChange={ handleOpenChange }
      size={{ lgDown: 'full', lg: 'md' }}
      closeOnInteractOutside={ false }
      modal={ false }
      trapFocus={ false }
      preventScroll={ false }
    >
      <DialogContent>
        <DialogHeader onBackToClick={ stepIndex !== 0 ? handleGoToPrevStep : undefined }>
          { step.title }
        </DialogHeader>
        <DialogBody mb={ 0 }>
          <Web3Provider>
            { step.content }
          </Web3Provider>
        </DialogBody>
      </DialogContent>
    </DialogRoot>
  );
};

export default React.memo(AddressVerificationModal);
