import { useMutation } from '@tanstack/react-query';
import React, { useState } from 'react';
import type { SubmitHandler } from 'react-hook-form';
import { FormProvider, useForm } from 'react-hook-form';

import type { AddressTag, AddressTagErrors } from 'types/api/account';

import type { ResourceErrorAccount } from 'lib/api/resources';
import useApiFetch from 'lib/api/useApiFetch';
import getErrorMessage from 'lib/getErrorMessage';
import { Button } from 'toolkit/chakra/button';
import { FormFieldAddress } from 'toolkit/components/forms/fields/FormFieldAddress';
import { FormFieldText } from 'toolkit/components/forms/fields/FormFieldText';

const TAG_MAX_LENGTH = 35;

type Props = {
  data?: Partial<AddressTag>;
  onOpenChange: ({ open }: { open: boolean }) => void;
  onSuccess: () => Promise<void>;
  setAlertVisible: (isAlertVisible: boolean) => void;
};

type Inputs = {
  address: string;
  tag: string;
};

const AddressForm: React.FC<Props> = ({ data, onOpenChange, onSuccess, setAlertVisible }) => {
  const apiFetch = useApiFetch();
  const [ pending, setPending ] = useState(false);
  const formApi = useForm<Inputs>({
    mode: 'onTouched',
    defaultValues: {
      address: data?.address_hash || '',
      tag: data?.name || '',
    },
  });

  const { mutateAsync } = useMutation({
    mutationFn: (formData: Inputs) => {
      const body = {
        name: formData?.tag,
        address_hash: formData?.address,
      };

      const isEdit = data?.id;
      if (isEdit) {
        return apiFetch('general:private_tags_address', {
          pathParams: { id: String(data.id) },
          fetchParams: { method: 'PUT', body },
        });
      }

      return apiFetch('general:private_tags_address', { fetchParams: { method: 'POST', body } });
    },
    onError: (error: ResourceErrorAccount<AddressTagErrors>) => {
      setPending(false);
      const errorMap = error.payload?.errors;
      if (errorMap?.address_hash || errorMap?.name) {
        errorMap?.address_hash && formApi.setError('address', { type: 'custom', message: getErrorMessage(errorMap, 'address_hash') });
        errorMap?.name && formApi.setError('tag', { type: 'custom', message: getErrorMessage(errorMap, 'name') });
      } else if (errorMap?.identity_id) {
        formApi.setError('address', { type: 'custom', message: getErrorMessage(errorMap, 'identity_id') });
      } else {
        setAlertVisible(true);
      }
    },
    onSuccess: async() => {
      await onSuccess();
      onOpenChange({ open: false });
      setPending(false);
    },
  });

  const onSubmit: SubmitHandler<Inputs> = async(formData) => {
    setAlertVisible(false);
    setPending(true);
    await mutateAsync(formData);
  };

  return (
    <FormProvider { ...formApi }>
      <form noValidate onSubmit={ formApi.handleSubmit(onSubmit) }>
        <FormFieldAddress<Inputs>
          name="address"
          required
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
        <Button
          type="submit"
          disabled={ !formApi.formState.isDirty }
          loading={ pending }
        >
          { data ? 'Save changes' : 'Add tag' }
        </Button>
      </form>
    </FormProvider>
  );
};

export default AddressForm;
