import { Box } from '@chakra-ui/react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import React, { useState } from 'react';
import type { SubmitHandler } from 'react-hook-form';
import { useForm, FormProvider } from 'react-hook-form';

import type { TransactionTag, TransactionTagErrors } from 'types/api/account';

import type { ResourceErrorAccount } from 'lib/api/resources';
import { resourceKey } from 'lib/api/resources';
import useApiFetch from 'lib/api/useApiFetch';
import getErrorMessage from 'lib/getErrorMessage';
import { Button } from 'toolkit/chakra/button';
import { FormFieldText } from 'toolkit/components/forms/fields/FormFieldText';
import { TRANSACTION_HASH_LENGTH, TRANSACTION_HASH_REGEXP } from 'toolkit/components/forms/validators/transaction';

const TAG_MAX_LENGTH = 35;

type Props = {
  data?: Partial<TransactionTag>;
  onOpenChange: ({ open }: { open: boolean }) => void;
  onSuccess: () => Promise<void>;
  setAlertVisible: (isAlertVisible: boolean) => void;
};

type Inputs = {
  transaction: string;
  tag: string;
};

const TransactionForm: React.FC<Props> = ({ data, onOpenChange, onSuccess, setAlertVisible }) => {
  const [ pending, setPending ] = useState(false);

  const formApi = useForm<Inputs>({
    mode: 'onTouched',
    defaultValues: {
      transaction: data?.transaction_hash || '',
      tag: data?.name || '',
    },
  });

  const queryClient = useQueryClient();
  const apiFetch = useApiFetch();

  const { mutateAsync } = useMutation({
    mutationFn: (formData: Inputs) => {
      const body = {
        name: formData?.tag,
        transaction_hash: formData?.transaction,
      };
      const isEdit = data?.id;

      if (isEdit) {
        return apiFetch('general:private_tags_tx', {
          pathParams: { id: String(data.id) },
          fetchParams: { method: 'PUT', body },
        });
      }

      return apiFetch('general:private_tags_tx', { fetchParams: { method: 'POST', body } });
    },
    onError: (error: ResourceErrorAccount<TransactionTagErrors>) => {
      setPending(false);
      const errorMap = error.payload?.errors;
      if (errorMap?.transaction_hash || errorMap?.name) {
        errorMap?.transaction_hash && formApi.setError('transaction', { type: 'custom', message: getErrorMessage(errorMap, 'transaction_hash') });
        errorMap?.name && formApi.setError('tag', { type: 'custom', message: getErrorMessage(errorMap, 'name') });
      } else if (errorMap?.identity_id) {
        formApi.setError('transaction', { type: 'custom', message: getErrorMessage(errorMap, 'identity_id') });
      } else {
        setAlertVisible(true);
      }
    },
    onSuccess: async() => {
      await queryClient.refetchQueries({ queryKey: [ resourceKey('general:private_tags_tx') ] });
      await onSuccess();
      onOpenChange({ open: false });
      setPending(false);
    },
  });

  const onSubmit: SubmitHandler<Inputs> = async(formData) => {
    setPending(true);
    await mutateAsync(formData);
  };

  return (
    <FormProvider { ...formApi }>
      <form noValidate onSubmit={ formApi.handleSubmit(onSubmit) }>
        <FormFieldText<Inputs>
          name="transaction"
          placeholder="Transaction hash (0x...)"
          required
          rules={{
            maxLength: TRANSACTION_HASH_LENGTH,
            pattern: TRANSACTION_HASH_REGEXP,
          }}
          bgColor="dialog.bg"
          mb={ 5 }
        />
        <FormFieldText<Inputs>
          name="tag"
          placeholder="Private tag (max 35 characters)"
          required
          rules={{
            maxLength: TAG_MAX_LENGTH,
          }}
          bgColor="dialog.bg"
          mb={ 8 }
        />
        <Box marginTop={ 8 }>
          <Button
            type="submit"
            disabled={ !formApi.formState.isDirty }
            loading={ pending }
          >
            { data ? 'Save changes' : 'Add tag' }
          </Button>
        </Box>
      </form>
    </FormProvider>
  );
};

export default TransactionForm;
