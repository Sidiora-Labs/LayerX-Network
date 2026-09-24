import { Box, Text } from '@chakra-ui/react';
import { useMutation } from '@tanstack/react-query';
import React, { useState } from 'react';
import type { SubmitHandler } from 'react-hook-form';
import { useForm, FormProvider } from 'react-hook-form';

import type { WatchlistAddress, WatchlistErrors } from 'types/api/account';

import type { ResourceErrorAccount } from 'lib/api/resources';
import useApiFetch from 'lib/api/useApiFetch';
import getErrorMessage from 'lib/getErrorMessage';
import { Alert } from 'toolkit/chakra/alert';
import { Button } from 'toolkit/chakra/button';
import { FormFieldAddress } from 'toolkit/components/forms/fields/FormFieldAddress';
import { FormFieldCheckbox } from 'toolkit/components/forms/fields/FormFieldCheckbox';
import { FormFieldText } from 'toolkit/components/forms/fields/FormFieldText';

import AddressFormNotifications, { NOTIFICATIONS } from './AddressFormNotifications';

const TAG_MAX_LENGTH = 35;

type Props = {
  data?: Partial<WatchlistAddress>;
  onSuccess: () => Promise<void>;
  setAlertVisible: (isAlertVisible: boolean) => void;
  isAdd: boolean;
  hasEmail: boolean;
  showEmailAlert?: boolean;
};

export type Inputs = {
  address: string;
  tag: string;
  notification: boolean;
  notification_settings: {
    [key: string]: {
      outcoming: boolean;
      incoming: boolean;
    };
  };
};

const AddressForm: React.FC<Props> = ({ data, onSuccess, setAlertVisible, isAdd, hasEmail, showEmailAlert }) => {
  const [ pending, setPending ] = useState(false);

  let notificationsDefault = {} as Inputs['notification_settings'];
  if (!data?.notification_settings) {
    NOTIFICATIONS.forEach((notificationType) => {
      notificationsDefault[notificationType] = { incoming: hasEmail, outcoming: hasEmail };
    });
  } else {
    notificationsDefault = data.notification_settings;
  }

  const formApi = useForm<Inputs>({
    defaultValues: {
      address: data?.address_hash || '',
      tag: data?.name || '',
      notification: data?.notification_methods ? data.notification_methods.email : hasEmail,
      notification_settings: notificationsDefault,
    },
    mode: 'onTouched',
  });

  const apiFetch = useApiFetch();

  function updateWatchlist(formData: Inputs) {
    const body = {
      name: formData?.tag,
      address_hash: formData?.address,
      notification_settings: formData.notification_settings,
      notification_methods: {
        email: formData.notification,
      },
    };
    if (!isAdd && data) {
      // edit address
      return apiFetch('general:watchlist', {
        pathParams: { id: data?.id ? String(data.id) : '' },
        fetchParams: { method: 'PUT', body },
      });

    } else {
      // add address
      return apiFetch('general:watchlist', { fetchParams: { method: 'POST', body } });
    }
  }

  const { mutateAsync } = useMutation({
    mutationFn: updateWatchlist,
    onSuccess: async() => {
      await onSuccess();
      setPending(false);
    },
    onError: (error: ResourceErrorAccount<WatchlistErrors>) => {
      setPending(false);
      const errorMap = error.payload?.errors;
      if (errorMap?.address_hash || errorMap?.name) {
        errorMap?.address_hash && formApi.setError('address', { type: 'custom', message: getErrorMessage(errorMap, 'address_hash') });
        errorMap?.name && formApi.setError('tag', { type: 'custom', message: getErrorMessage(errorMap, 'name') });
      } else if (errorMap?.watchlist_id) {
        formApi.setError('address', { type: 'custom', message: getErrorMessage(errorMap, 'watchlist_id') });
      } else {
        setAlertVisible(true);
      }
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
        { hasEmail ? (
          <>
            <Text color="text.secondary" fontSize="sm" marginBottom={ 5 }>
              Please select what types of notifications you will receive
            </Text>
            <Box marginBottom={ 8 }>
              <AddressFormNotifications/>
            </Box>
            <Text color="text.secondary" fontSize="sm" marginBottom={{ base: '10px', lg: 5 }}>Notification methods</Text>
            <FormFieldCheckbox<Inputs, 'notification'>
              name="notification"
              label="Email notifications"
            />
          </>
        ) : null }
        { !hasEmail && showEmailAlert ? (
          <Alert
            status="info"
            descriptionProps={{ alignItems: 'center', gap: 2 }}
            w="fit-content"
            mb={ 6 }
          >
            To receive notifications you need to add an email to your profile.
          </Alert>
        ) : null }
        <Button
          type="submit"
          loading={ pending }
          disabled={ !formApi.formState.isDirty }
          mt={ 8 }
        >
          { !isAdd ? 'Save changes' : 'Add address' }
        </Button>
      </form>
    </FormProvider>

  );
};

export default AddressForm;
