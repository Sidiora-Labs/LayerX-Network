import { chakra, Grid, GridItem } from '@chakra-ui/react';
import { useRouter } from 'next/router';
import React from 'react';
import type { SubmitHandler } from 'react-hook-form';
import { useForm, FormProvider } from 'react-hook-form';

import type { FormFields, FormSubmitResult } from './types';
import type { UserInfo } from 'types/api/account';
import type { PublicTagTypesResponse } from 'types/api/addressMetadata';

import appConfig from 'configs/app';
import useApiFetch from 'lib/api/useApiFetch';
import getErrorObj from 'lib/errors/getErrorObj';
import getErrorObjPayload from 'lib/errors/getErrorObjPayload';
import useIsMobile from 'lib/hooks/useIsMobile';
import { Button } from 'toolkit/chakra/button';
import { Heading } from 'toolkit/chakra/heading';
import { FormFieldEmail } from 'toolkit/components/forms/fields/FormFieldEmail';
import { FormFieldText } from 'toolkit/components/forms/fields/FormFieldText';
import { FormFieldUrl } from 'toolkit/components/forms/fields/FormFieldUrl';
import { Hint } from 'toolkit/components/Hint/Hint';
import ReCaptcha from 'ui/shared/reCaptcha/ReCaptcha';
import useReCaptcha from 'ui/shared/reCaptcha/useReCaptcha';

import PublicTagsSubmitFieldAddresses from './fields/PublicTagsSubmitFieldAddresses';
import PublicTagsSubmitFieldTags from './fields/PublicTagsSubmitFieldTags';
import { convertFormDataToRequestsBody, getFormDefaultValues } from './utils';

interface Props {
  config?: PublicTagTypesResponse | undefined;
  userInfo?: UserInfo | undefined;
  onSubmitResult: (result: FormSubmitResult) => void;
}

const PublicTagsSubmitForm = ({ config, userInfo, onSubmitResult }: Props) => {
  const isMobile = useIsMobile();
  const router = useRouter();
  const apiFetch = useApiFetch();
  const recaptcha = useReCaptcha();

  const formApi = useForm<FormFields>({
    mode: 'onBlur',
    defaultValues: getFormDefaultValues(router.query, userInfo),
  });

  React.useEffect(() => {
    if (
      router.query.addresses ||
      router.query.requesterName ||
      router.query.requesterEmail ||
      router.query.companyName ||
      router.query.companyWebsite
    ) {
      router.replace({ pathname: '/public-tags/submit' }, undefined, { shallow: true });
    }
  }, [ router ]);

  const onFormSubmit: SubmitHandler<FormFields> = React.useCallback(async(data) => {

    const token = await recaptcha.executeAsync();

    if (!token) {
      throw new Error('ReCaptcha is not solved');
    }

    const requestsBody = convertFormDataToRequestsBody(data);

    const result = await Promise.all(requestsBody.map(async(body) => {
      return apiFetch<'admin:public_tag_application', unknown, { message: string }>(
        'admin:public_tag_application', {
          pathParams: { chainId: appConfig.chain.id },
          fetchParams: {
            method: 'POST',
            body: { submission: body },
            headers: {
              'recaptcha-v2-response': token,
            },
          },
        })
        .then(() => ({ error: null, payload: body }))
        .catch((error: unknown) => {
          const errorObj = getErrorObj(error);
          const messageFromPayload = getErrorObjPayload<{ message?: string }>(errorObj)?.message;
          const messageFromError = errorObj && 'message' in errorObj && typeof errorObj.message === 'string' ? errorObj.message : undefined;
          const message = messageFromPayload || messageFromError || 'Something went wrong.';
          return { error: message, payload: body };
        });
    }));

    onSubmitResult(result);
  }, [ apiFetch, onSubmitResult, recaptcha ]);

  if (!appConfig.services.reCaptchaV2.siteKey) {
    return null;
  }

  return (
    <FormProvider { ...formApi }>
      <chakra.form
        noValidate
        onSubmit={ formApi.handleSubmit(onFormSubmit) }
      >
        <Grid
          columnGap={ 3 }
          rowGap={ 3 }
          templateColumns={{ base: '1fr', lg: '1fr 1fr minmax(0, 200px)', xl: '1fr 1fr minmax(0, 250px)' }}
        >
          <GridItem colSpan={{ base: 1, lg: 3 }}>
            <Heading level="2">
              Company info
            </Heading>
          </GridItem>
          <FormFieldText<FormFields> name="requesterName" required placeholder="Your name"/>
          <FormFieldEmail<FormFields> name="requesterEmail" required/>

          { !isMobile && <div/> }
          <FormFieldText<FormFields> name="companyName" placeholder="Company name"/>
          <FormFieldUrl<FormFields> name="companyWebsite" placeholder="Company website"/>
          { !isMobile && <div/> }

          <GridItem colSpan={{ base: 1, lg: 3 }} mt={{ base: 3, lg: 5 }}>
            <Heading level="2" display="flex" alignItems="center" columnGap={ 1 }>
              Public tags/labels
              <Hint label="Submit a public tag proposal for our moderation team to review"/>
            </Heading>
          </GridItem>
          <PublicTagsSubmitFieldAddresses/>
          <PublicTagsSubmitFieldTags tagTypes={ config?.tagTypes }/>
          <GridItem colSpan={{ base: 1, lg: 2 }}>
            <FormFieldText<FormFields>
              name="description"
              required
              placeholder={
                isMobile ?
                  'Confirm the connection between addresses and tags' :
                  'Provide a comment to confirm the connection between addresses and tags (max 500 characters)'
              }
              maxH="160px"
              rules={{ maxLength: 500 }}
              asComponent="Textarea"
              size="2xl"
            />
          </GridItem>

          <GridItem colSpan={{ base: 1, lg: 2 }}>
            <ReCaptcha { ...recaptcha }/>
          </GridItem>
          { !isMobile && <div/> }

          <Button
            variant="solid"
            type="submit"
            mt={ 3 }
            loading={ formApi.formState.isSubmitting }
            loadingText="Send request"
            w="min-content"
            disabled={ recaptcha.isInitError }
          >
            Send request
          </Button>
        </Grid>
      </chakra.form>
    </FormProvider>
  );
};

export default React.memo(PublicTagsSubmitForm);
