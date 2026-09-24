import { Box } from '@chakra-ui/react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import React, { useCallback } from 'react';
import type { SubmitHandler } from 'react-hook-form';
import { useForm, FormProvider } from 'react-hook-form';

import type { CustomAbi, CustomAbis, CustomAbiErrors } from 'types/api/account';

import type { ResourceErrorAccount } from 'lib/api/resources';
import { resourceKey } from 'lib/api/resources';
import useApiFetch from 'lib/api/useApiFetch';
import getErrorMessage from 'lib/getErrorMessage';
import { Button } from 'toolkit/chakra/button';
import { FormFieldAddress } from 'toolkit/components/forms/fields/FormFieldAddress';
import { FormFieldText } from 'toolkit/components/forms/fields/FormFieldText';

export type FormData = CustomAbi | {
  contract_address_hash: string;
  name: string;
} | undefined;

type Props = {
  data: FormData;
  onOpenChange: ({ open }: { open: boolean }) => void;
  onSuccess?: () => Promise<void>;
  setAlertVisible: (isAlertVisible: boolean) => void;
};

type Inputs = {
  contract_address_hash: string;
  name: string;
  abi: string;
};

const NAME_MAX_LENGTH = 255;

const CustomAbiForm: React.FC<Props> = ({ data, onOpenChange, onSuccess, setAlertVisible }) => {
  const formApi = useForm<Inputs>({
    defaultValues: {
      contract_address_hash: data?.contract_address_hash || '',
      name: data?.name || '',
      abi: data && 'abi' in data ? JSON.stringify(data.abi) : '',
    },
    mode: 'onTouched',
  });

  const queryClient = useQueryClient();
  const apiFetch = useApiFetch();

  const customAbiKey = (data: Inputs & { id?: string }) => {
    const body = { name: data.name, contract_address_hash: data.contract_address_hash, abi: data.abi };

    if (!data.id) {
      return apiFetch('general:custom_abi', { fetchParams: { method: 'POST', body } });
    }

    return apiFetch('general:custom_abi', {
      pathParams: { id: String(data.id) },
      fetchParams: { method: 'PUT', body },
    });
  };

  const { mutateAsync, isPending } = useMutation({
    mutationFn: customAbiKey,
    onSuccess: async(data) => {
      const response = data as unknown as CustomAbi;
      queryClient.setQueryData([ resourceKey('general:custom_abi') ], (prevData: CustomAbis | undefined) => {
        const isExisting = prevData && prevData.some((item) => item.id === response.id);

        if (isExisting) {
          return prevData.map((item) => {
            if (item.id === response.id) {
              return response;
            }

            return item;
          });
        }

        return [ response, ...(prevData || []) ];
      });
      await onSuccess?.();
      onOpenChange({ open: false });
    },
    onError: (error: ResourceErrorAccount<CustomAbiErrors>) => {
      const errorMap = error.payload?.errors;
      if (errorMap?.address_hash || errorMap?.name || errorMap?.abi) {
        errorMap?.address_hash && formApi.setError('contract_address_hash', { type: 'custom', message: getErrorMessage(errorMap, 'address_hash') });
        errorMap?.name && formApi.setError('name', { type: 'custom', message: getErrorMessage(errorMap, 'name') });
        errorMap?.abi && formApi.setError('abi', { type: 'custom', message: getErrorMessage(errorMap, 'abi') });
      } else if (errorMap?.identity_id) {
        formApi.setError('contract_address_hash', { type: 'custom', message: getErrorMessage(errorMap, 'identity_id') });
      } else {
        setAlertVisible(true);
      }
    },
  });

  const onSubmit: SubmitHandler<Inputs> = useCallback(async(formData) => {
    setAlertVisible(false);
    const id = data && 'id' in data ? String(data.id) : undefined;
    await mutateAsync({ ...formData, id });
  }, [ mutateAsync, data, setAlertVisible ]);

  return (
    <FormProvider { ...formApi }>
      <form noValidate onSubmit={ formApi.handleSubmit(onSubmit) }>
        <FormFieldAddress<Inputs>
          name="contract_address_hash"
          placeholder="Smart contract address (0x...)"
          required
          bgColor="dialog.bg"
          readOnly={ Boolean(data && 'contract_address_hash' in data) }
          mb={ 5 }
        />
        <FormFieldText<Inputs>
          name="name"
          placeholder="Project name"
          required
          rules={{
            maxLength: NAME_MAX_LENGTH,
          }}
          bgColor="dialog.bg"
          mb={ 5 }
        />
        <FormFieldText<Inputs>
          name="abi"
          placeholder="Custom ABI [{...}] (JSON format)"
          required
          asComponent="Textarea"
          bgColor="dialog.bg"
          size="2xl"
          minH="300px"
          mb={ 8 }
        />
        <Box>
          <Button
            type="submit"
            disabled={ !formApi.formState.isDirty }
            loading={ isPending }
          >
            { data && 'id' in data ? 'Save' : 'Create custom ABI' }
          </Button>
        </Box>
      </form>
    </FormProvider>
  );
};

export default React.memo(CustomAbiForm);
