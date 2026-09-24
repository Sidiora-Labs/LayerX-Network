import { chakra, Box, Text } from '@chakra-ui/react';
import React from 'react';
import type { SubmitHandler } from 'react-hook-form';
import { FormProvider, useForm } from 'react-hook-form';

import type { OtpCodeFormFields, ScreenSuccess } from '../types';
import type { UserInfo } from 'types/api/account';

import useApiFetch from 'lib/api/useApiFetch';
import getErrorMessage from 'lib/errors/getErrorMessage';
import getErrorObjPayload from 'lib/errors/getErrorObjPayload';
import { Button } from 'toolkit/chakra/button';
import { toaster } from 'toolkit/chakra/toaster';
import IconSvg from 'ui/shared/IconSvg';
import ReCaptcha from 'ui/shared/reCaptcha/ReCaptcha';
import useReCaptcha from 'ui/shared/reCaptcha/useReCaptcha';

import AuthModalFieldOtpCode from '../fields/AuthModalFieldOtpCode';

interface Props {
  email: string;
  onSuccess: (screen: ScreenSuccess) => void;
  isAuth?: boolean;
}

const AuthModalScreenOtpCode = ({ email, onSuccess, isAuth }: Props) => {

  const apiFetch = useApiFetch();
  const recaptcha = useReCaptcha();
  const [ isCodeSending, setIsCodeSending ] = React.useState(false);

  const formApi = useForm<OtpCodeFormFields>({
    mode: 'onBlur',
    defaultValues: {
      code: [],
    },
  });

  const onFormSubmit: SubmitHandler<OtpCodeFormFields> = React.useCallback((formData) => {
    const resource = isAuth ? 'general:auth_link_email' : 'general:auth_confirm_otp';
    return apiFetch<typeof resource, UserInfo, unknown>(resource, {
      fetchParams: {
        method: 'POST',
        body: {
          otp: formData.code.join(''),
          email,
        },
      },
    })
      .then((response) => {
        if (!('name' in response)) {
          throw Error('Something went wrong');
        }
        onSuccess({ type: 'success_email', email, isAuth, profile: response });
      })
      .catch((error) => {
        const apiError = getErrorObjPayload<{ message: string }>(error);

        if (apiError?.message) {
          formApi.setError('code', { message: apiError.message });
          return;
        }

        toaster.error({
          title: 'Error',
          description: getErrorMessage(error) || 'Something went wrong',
        });
      });
  }, [ apiFetch, email, onSuccess, isAuth, formApi ]);

  const resendCodeFetchFactory = React.useCallback((recaptchaToken?: string) => {
    return apiFetch('general:auth_send_otp', {
      fetchParams: {
        method: 'POST',
        body: { email, recaptcha_response: recaptchaToken },
        headers: {
          ...(recaptchaToken && { 'recaptcha-v2-response': recaptchaToken }),
        },
      },
    });
  }, [ apiFetch, email ]);

  const handleResendCodeClick = React.useCallback(async() => {
    try {
      formApi.clearErrors('code');
      setIsCodeSending(true);
      await recaptcha.fetchProtectedResource(resendCodeFetchFactory);

      toaster.success({
        title: 'Success',
        description: 'Code has been sent to your email',
      });
    } catch (error) {
      const apiError = getErrorObjPayload<{ message: string }>(error);

      toaster.error({
        title: 'Error',
        description: apiError?.message || getErrorMessage(error) || 'Something went wrong',
      });
    } finally {
      setIsCodeSending(false);
    }
  }, [ formApi, recaptcha, resendCodeFetchFactory ]);

  return (
    <FormProvider { ...formApi }>
      <chakra.form
        noValidate
        onSubmit={ formApi.handleSubmit(onFormSubmit) }
      >
        <Text mb={ 6 }>
          Please check{ ' ' }
          <chakra.span fontWeight="700">{ email }</chakra.span>{ ' ' }
          and enter your code below.
        </Text>
        <AuthModalFieldOtpCode isDisabled={ isCodeSending }/>
        <Button
          variant="link"
          columnGap={ 2 }
          mt={ 3 }
          disabled={ isCodeSending || recaptcha.isInitError }
          onClick={ handleResendCodeClick }
        >
          <IconSvg name="repeat" boxSize={ 5 }/>
          <Box fontSize="sm">Resend code</Box>
        </Button>
        <ReCaptcha { ...recaptcha }/>
        <Button
          mt={ 6 }
          type="submit"
          loading={ formApi.formState.isSubmitting }
          disabled={ formApi.formState.isSubmitting || isCodeSending || recaptcha.isInitError }
          loadingText="Submit"
          onClick={ formApi.handleSubmit(onFormSubmit) }
        >
          Submit
        </Button>
      </chakra.form>
    </FormProvider>
  );
};

export default React.memo(AuthModalScreenOtpCode);
