import { Box, Flex, chakra } from '@chakra-ui/react';
import React from 'react';
import type { SubmitHandler } from 'react-hook-form';
import { useForm, FormProvider } from 'react-hook-form';
import { encodeFunctionData, type AbiFunction } from 'viem';

import type { FormSubmitHandler, FormSubmitResult, MethodCallStrategy, SmartContractMethod } from '../types';

import config from 'configs/app';
import * as mixpanel from 'lib/mixpanel/index';
import { Button } from 'toolkit/chakra/button';
import { Tooltip } from 'toolkit/chakra/tooltip';
import { useDisclosure } from 'toolkit/hooks/useDisclosure';
import { SECOND } from 'toolkit/utils/consts';
import IconSvg from 'ui/shared/IconSvg';

import { isReadMethod, isWriteMethod } from '../utils';
import ContractMethodFieldAccordion from './ContractMethodFieldAccordion';
import ContractMethodFieldInput from './ContractMethodFieldInput';
import ContractMethodFieldInputArray from './ContractMethodFieldInputArray';
import ContractMethodFieldInputTuple from './ContractMethodFieldInputTuple';
import ContractMethodOutput from './ContractMethodOutput';
import ContractMethodResultPublicClient from './ContractMethodResultPublicClient';
import ContractMethodResultWalletClient from './ContractMethodResultWalletClient';
import { getFieldLabel, matchArray, transformFormDataToMethodArgs } from './utils';
import type { ContractMethodFormFields } from './utils';

// eslint-disable-next-line max-len
const NO_WALLET_CLIENT_TEXT = 'Blockchain interaction is not available at the moment since a wallet connector is not configured for this application. Please contact the service maintainer to make necessary changes in the service configuration.';

interface Props {
  data: SmartContractMethod;
  attempt: number;
  onSubmit: FormSubmitHandler;
  isOpen: boolean;
  onReset: () => void;
}

const ContractMethodForm = ({ data, attempt, onSubmit, onReset, isOpen }: Props) => {

  const [ result, setResult ] = React.useState<FormSubmitResult>();
  const [ isLoading, setLoading ] = React.useState(false);
  const [ callStrategy, setCallStrategy ] = React.useState<MethodCallStrategy>();
  const callStrategyRef = React.useRef(callStrategy);

  const formApi = useForm<ContractMethodFormFields>({
    mode: 'all',
    shouldUnregister: true,
  });

  const calldataButtonTooltip = useDisclosure();

  const handleButtonClick = React.useCallback((event: React.MouseEvent) => {
    const callStrategy = event?.currentTarget.getAttribute('data-call-strategy');
    setCallStrategy(callStrategy as MethodCallStrategy);
    callStrategyRef.current = callStrategy as MethodCallStrategy;

    if (callStrategy === 'copy_calldata') {
      calldataButtonTooltip.onOpen();
      window.setTimeout(() => {
        calldataButtonTooltip.onClose();
      }, SECOND);
    }
  }, [ calldataButtonTooltip ]);

  const methodType = isReadMethod(data) ? 'read' : 'write';

  const onFormSubmit: SubmitHandler<ContractMethodFormFields> = React.useCallback(async(formData) => {
    const args = transformFormDataToMethodArgs(formData);

    if (callStrategyRef.current === 'copy_calldata') {
      if (!('inputs' in data)) {
        return;
      }

      // since we have added additional input for native coin value
      // we need to slice it off
      const argsToPass = args.slice(0, data.inputs.length);

      if (!('name' in data)) {
        // this condition means that the fallback method acts as a read method with inputs
        const data = typeof argsToPass[0] === 'string' && argsToPass[0].startsWith('0x') ? argsToPass[0] as `0x${ string }` : '0x';
        await navigator.clipboard.writeText(data);
        return;
      }

      const callData = encodeFunctionData({
        abi: [ data ],
        functionName: data.name,
        args: argsToPass,
      });
      await navigator.clipboard.writeText(callData);
      return;
    }

    setResult(undefined);
    setLoading(true);

    onSubmit(data, args, callStrategyRef.current)
      .then((result) => {
        setResult(result);
      })
      .catch((error: Error) => {
        setResult({
          source: callStrategyRef.current === 'write' ? 'wallet_client' : 'public_client',
          data: error,
        });
        setLoading(false);
      })
      .finally(() => {
        mixpanel.logEvent(mixpanel.EventTypes.CONTRACT_INTERACTION, {
          'Method type': methodType === 'write' ? 'Write' : 'Read',
          'Method name': 'name' in data ? data.name : 'Fallback',
        });
      });
  }, [ data, methodType, onSubmit ]);

  React.useEffect(() => {
    if (isOpen && !callStrategyRef.current && attempt === 0) {
      const hasConstantOutputs = isReadMethod(data) && data.inputs.length === 0;
      if (hasConstantOutputs) {
        setCallStrategy('read');
        callStrategyRef.current = 'read';
        onFormSubmit({});
      }
    }
  }, [ data, isOpen, onFormSubmit, attempt ]);

  const handleResultSettle = React.useCallback(() => {
    setLoading(false);
  }, []);

  const handleFormChange = React.useCallback(() => {
    result && setResult(undefined);
  }, [ result ]);

  const inputs: AbiFunction['inputs'] = React.useMemo(() => {
    return [
      ...('inputs' in data && data.inputs ? data.inputs : []),
      ...('stateMutability' in data && data.stateMutability === 'payable' ? [ {
        name: `Send native ${ config.chain.currency.symbol || 'coin' }`,
        type: 'uint256' as const,
        internalType: 'uint256' as const,
        fieldType: 'native_coin' as const,
      } ] : []),
    ];
  }, [ data ]);

  const primaryButton = (() => {
    const isDisabled = !config.features.blockchainInteraction.isEnabled && methodType === 'write';
    const text = methodType === 'write' ? 'Write' : 'Read';
    const buttonCallStrategy = methodType === 'write' ? 'write' : 'read';

    return (
      <Tooltip content={ NO_WALLET_CLIENT_TEXT } disabled={ !isDisabled }>
        <Button
          loading={ callStrategy === buttonCallStrategy && isLoading }
          disabled={ isLoading || isDisabled }
          onClick={ handleButtonClick }
          loadingText={ text }
          variant="outline"
          size="sm"
          flexShrink={ 0 }
          width="min-content"
          px={ 4 }
          type="submit"
          data-call-strategy={ buttonCallStrategy }
        >
          { text }
        </Button>
      </Tooltip>
    );
  })();

  const secondaryButton = (() => {
    if (methodType === 'read') {
      return null;
    }

    const text = 'Simulate';
    const buttonCallStrategy = 'simulate';

    return (
      <Button
        loading={ callStrategy === buttonCallStrategy && isLoading }
        disabled={ isLoading }
        onClick={ handleButtonClick }
        loadingText={ text }
        variant="outline"
        size="sm"
        flexShrink={ 0 }
        width="min-content"
        px={ 4 }
        type="submit"
        data-call-strategy={ buttonCallStrategy }
      >
        { text }
      </Button>
    );
  })();

  const copyCallDataButton = (() => {
    if (inputs.length === 0) {
      return null;
    }

    if (inputs.length === 1) {
      const [ input ] = inputs;
      if ('fieldType' in input && input.fieldType === 'native_coin') {
        return null;
      }
    }

    const text = 'Copy calldata';
    const buttonCallStrategy = 'copy_calldata';
    const isDisabled = isLoading || !formApi.formState.isValid;

    return (
      <Tooltip
        disabled={ isDisabled }
        content="Copied"
        closeDelay={ SECOND }
        open={ calldataButtonTooltip.open }
      >
        <Button
          loading={ callStrategy === buttonCallStrategy && isLoading }
          disabled={ isDisabled }
          onClick={ handleButtonClick }
          loadingText={ text }
          variant="outline"
          size="sm"
          flexShrink={ 0 }
          width="min-content"
          px={ 4 }
          type="submit"
          data-call-strategy={ buttonCallStrategy }
        >
          { text }
        </Button>
      </Tooltip>
    );
  })();

  const showOutputResult = result && result.source === 'public_client' && !(result.data instanceof Error);

  return (
    <Box>
      <FormProvider { ...formApi }>
        <chakra.form
          noValidate
          onSubmit={ formApi.handleSubmit(onFormSubmit) }
          onChange={ handleFormChange }
        >
          <Flex flexDir="column" rowGap={ 3 } mb={ 6 } _empty={{ display: 'none' }}>
            { inputs.map((input, index) => {
              const props = {
                data: input,
                basePath: `${ index }`,
                isDisabled: isLoading,
                level: 0,
                isOptional: data.type === 'fallback' && isWriteMethod(data),
              };

              if ('components' in input && input.components && input.type === 'tuple') {
                return <ContractMethodFieldInputTuple key={ index } { ...props }/>;
              }

              const arrayMatch = matchArray(input.type);
              if (arrayMatch) {
                if (arrayMatch.isNested) {
                  const fieldsWithErrors = Object.keys(formApi.formState.errors);
                  const isInvalid = fieldsWithErrors.some((field) => field.startsWith(index + ':'));

                  return (
                    <ContractMethodFieldAccordion
                      key={ index }
                      level={ 0 }
                      label={ getFieldLabel(input) }
                      isInvalid={ isInvalid }
                    >
                      <ContractMethodFieldInputArray { ...props }/>
                    </ContractMethodFieldAccordion>
                  );

                }

                return <ContractMethodFieldInputArray key={ index } { ...props }/>;
              }

              return <ContractMethodFieldInput key={ index } { ...props } path={ `${ index }` }/>;
            }) }
          </Flex>
          <Flex flexDir="row" gap={ 3 } flexWrap="wrap">
            { secondaryButton }
            { primaryButton }
            { copyCallDataButton }
            { result && !isLoading && (
              <Button
                variant="link"
                size="sm"
                onClick={ onReset }
                gap={ 1 }
              >
                <IconSvg name="repeat" boxSize={ 5 }/>
                Reset
              </Button>
            ) }
          </Flex>
        </chakra.form>
      </FormProvider>
      <Flex flexDir="column" rowGap={{ base: 1, lg: 2 }} mt={ 3 } _empty={{ display: 'none' }}>
        { result && result.source === 'wallet_client' && (
          <ContractMethodResultWalletClient
            data={ result.data }
            onSettle={ handleResultSettle }
          />
        ) }
        { result && result.source === 'public_client' && (
          <ContractMethodResultPublicClient
            data={ result.data }
            estimatedGas={ result.estimatedGas }
            onSettle={ handleResultSettle }
          />
        ) }
        { 'outputs' in data && data.outputs.length > 0 && (
          <ContractMethodOutput
            data={ showOutputResult ? result.data : undefined }
            onSettle={ handleResultSettle }
            abiItem={ data }
            mode={ showOutputResult ? 'result' : 'preview' }
          />
        ) }
      </Flex>
    </Box>
  );
};

export default React.memo(ContractMethodForm) as typeof ContractMethodForm;
