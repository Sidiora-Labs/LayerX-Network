import { Grid, Text, chakra } from '@chakra-ui/react';
import React from 'react';
import type { SubmitHandler } from 'react-hook-form';
import { useForm, FormProvider } from 'react-hook-form';

import type { FormFields } from './types';
import type { SocketMessage } from 'lib/socket/types';
import type { SmartContract, SmartContractVerificationMethodApi } from 'types/api/contract';
import type { SmartContractVerificationConfig } from 'types/client/contract';

import { route } from 'nextjs-routes';

import useApiFetch from 'lib/api/useApiFetch';
import capitalizeFirstLetter from 'lib/capitalizeFirstLetter';
import delay from 'lib/delay';
import getErrorObjStatusCode from 'lib/errors/getErrorObjStatusCode';
import useRewardsActivity from 'lib/hooks/useRewardsActivity';
import * as mixpanel from 'lib/mixpanel/index';
import useSocketChannel from 'lib/socket/useSocketChannel';
import useSocketMessage from 'lib/socket/useSocketMessage';
import { Button } from 'toolkit/chakra/button';
import { toaster } from 'toolkit/chakra/toaster';
import { useUpdateEffect } from 'toolkit/hooks/useUpdateEffect';

import ContractVerificationFieldAddress from './fields/ContractVerificationFieldAddress';
import ContractVerificationFieldLicenseType from './fields/ContractVerificationFieldLicenseType';
import ContractVerificationFieldMethod from './fields/ContractVerificationFieldMethod';
import ContractVerificationFlattenSourceCode from './methods/ContractVerificationFlattenSourceCode';
import ContractVerificationMultiPartFile from './methods/ContractVerificationMultiPartFile';
import ContractVerificationSolidityFoundry from './methods/ContractVerificationSolidityFoundry';
import ContractVerificationSolidityHardhat from './methods/ContractVerificationSolidityHardhat';
import ContractVerificationSourcify from './methods/ContractVerificationSourcify';
import ContractVerificationStandardInput from './methods/ContractVerificationStandardInput';
import ContractVerificationStylusGitHubRepo from './methods/ContractVerificationStylusGitHubRepo';
import ContractVerificationVyperContract from './methods/ContractVerificationVyperContract';
import ContractVerificationVyperMultiPartFile from './methods/ContractVerificationVyperMultiPartFile';
import ContractVerificationVyperStandardInput from './methods/ContractVerificationVyperStandardInput';
import { prepareRequestBody, formatSocketErrors, getDefaultValues, METHOD_LABELS } from './utils';

interface Props {
  method?: SmartContractVerificationMethodApi;
  config: SmartContractVerificationConfig;
  hash?: string;
}

const ContractVerificationForm = ({ method: methodFromQuery, config, hash }: Props) => {
  const formApi = useForm<FormFields>({
    mode: 'onBlur',
    defaultValues: getDefaultValues(methodFromQuery, config, hash, []),
  });
  const { handleSubmit, watch, formState, setError, reset, getFieldState, getValues, clearErrors } = formApi;
  const submitPromiseResolver = React.useRef<((value: unknown) => void)>(undefined);
  const methodNameRef = React.useRef<string>(undefined);

  const apiFetch = useApiFetch();
  const { trackContract } = useRewardsActivity();
  const onFormSubmit: SubmitHandler<FormFields> = React.useCallback(async(data) => {
    const body = prepareRequestBody(data);

    if (!hash) {
      try {
        const response = await apiFetch<'general:contract', SmartContract>('general:contract', {
          pathParams: { hash: data.address.toLowerCase() },
        });

        const isVerifiedContract = 'is_verified' in response && response?.is_verified && !response.is_partially_verified;
        if (isVerifiedContract) {
          setError('address', { message: 'Contract has already been verified' });
          return Promise.resolve();
        }
      } catch (error) {
        const statusCode = getErrorObjStatusCode(error);
        const message = statusCode === 404 ? 'Address is not a smart contract' : 'Something went wrong';
        setError('address', { message });
        return Promise.resolve();
      }
    }

    try {
      await trackContract(data.address);
      await apiFetch('general:contract_verification_via', {
        pathParams: { method: data.method[0], hash: data.address.toLowerCase() },
        fetchParams: {
          method: 'POST',
          body,
        },
      });
    } catch (error) {
      return;
    }

    return new Promise((resolve) => {
      submitPromiseResolver.current = resolve;
    });
  }, [ apiFetch, hash, setError, trackContract ]);

  const handleFormChange = React.useCallback(() => {
    clearErrors('root');
  }, [ clearErrors ]);

  const address = watch('address');
  const addressState = getFieldState('address');

  const handleNewSocketMessage: SocketMessage.ContractVerification['handler'] = React.useCallback(async(payload) => {
    if (payload.status === 'error') {
      const errors = formatSocketErrors(payload.errors);

      const existingErrors = errors.filter(Boolean).filter(([ field ]) => getValues(field));
      if (existingErrors.length) {
        existingErrors.forEach(([ field, error ]) => setError(field, error));
      } else {
        const globalErrors = Object.entries(payload.errors).map(([ , value ]) => value.join(', '));
        const rootError = capitalizeFirstLetter(globalErrors.join('\n\n'));
        setError('root', { message: rootError });
      }

      await delay(100); // have to wait a little bit, otherwise isSubmitting status will not be updated
      submitPromiseResolver.current?.(null);
      return;
    }

    toaster.success({
      title: 'Success',
      description: 'Contract is successfully verified.',
    });

    mixpanel.logEvent(
      mixpanel.EventTypes.CONTRACT_VERIFICATION,
      { Status: 'Finished', Method: methodNameRef.current || '' },
      { send_immediately: true },
    );

    window.location.assign(route({ pathname: '/address/[hash]', query: { hash: address, tab: 'contract' } }));
  }, [ setError, address, getValues ]);

  const handleSocketError = React.useCallback(() => {
    if (!formState.isSubmitting) {
      return;
    }

    submitPromiseResolver.current?.(null);

    toaster.error({
      title: 'Error',
      description: 'There was an error with socket connection. Try again later.',
    });
  // callback should not change when form is submitted
  // otherwise it will resubscribe to channel, but we don't want that since in that case we might miss verification result message
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ ]);

  const channel = useSocketChannel({
    topic: `addresses:${ address?.toLowerCase() }`,
    onSocketClose: handleSocketError,
    onSocketError: handleSocketError,
    isDisabled: !address || Boolean(address && addressState.error),
  });
  useSocketMessage({
    channel,
    event: 'verification_result',
    handler: handleNewSocketMessage,
  });

  const methods = React.useMemo(() => {
    return {
      'flattened-code': <ContractVerificationFlattenSourceCode config={ config }/>,
      'standard-input': <ContractVerificationStandardInput config={ config }/>,
      sourcify: <ContractVerificationSourcify/>,
      'multi-part': <ContractVerificationMultiPartFile config={ config }/>,
      'vyper-code': <ContractVerificationVyperContract config={ config }/>,
      'vyper-multi-part': <ContractVerificationVyperMultiPartFile config={ config }/>,
      'vyper-standard-input': <ContractVerificationVyperStandardInput config={ config }/>,
      'solidity-hardhat': <ContractVerificationSolidityHardhat config={ config }/>,
      'solidity-foundry': <ContractVerificationSolidityFoundry/>,
      'stylus-github-repository': <ContractVerificationStylusGitHubRepo config={ config }/>,
    };
  }, [ config ]);
  const method = watch('method');
  const methodValue = method?.[0];
  const licenseType = watch('license_type');
  const content = methods[methodValue] || null;

  useUpdateEffect(() => {
    if (methodValue) {
      reset(getDefaultValues(methodValue, config, hash || address, licenseType));

      const methodName = METHOD_LABELS[methodValue];
      mixpanel.logEvent(mixpanel.EventTypes.CONTRACT_VERIFICATION, { Status: 'Method selected', Method: methodName });
      methodNameRef.current = methodName;
    }
  // !!! should run only when method is changed
  }, [ methodValue ]);

  return (
    <FormProvider { ...formApi }>
      <chakra.form
        noValidate
        onSubmit={ handleSubmit(onFormSubmit) }
        onChange={ handleFormChange }
      >
        <Grid as="section" columnGap="30px" rowGap={{ base: 2, lg: 5 }} templateColumns={{ base: '1fr', lg: 'minmax(auto, 680px) minmax(0, 340px)' }}>
          { !hash && <ContractVerificationFieldAddress/> }
          <ContractVerificationFieldLicenseType/>
          <ContractVerificationFieldMethod methods={ config.verification_options }/>
        </Grid>
        { content }
        { formState.errors.root?.message && <Text color="text.error" mt={ 4 } fontSize="sm" whiteSpace="pre-wrap">{ formState.errors.root.message }</Text> }
        { Boolean(method) && methodValue !== 'solidity-hardhat' && methodValue !== 'solidity-foundry' && methodValue !== 'sourcify' && (
          <Button
            size="md"
            type="submit"
            mt={ 12 }
            loading={ formState.isSubmitting }
            loadingText="Verify & publish"
          >
            Verify & publish
          </Button>
        ) }
      </chakra.form>
    </FormProvider>
  );
};

export default React.memo(ContractVerificationForm);
