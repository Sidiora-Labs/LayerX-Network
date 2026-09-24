import { Text } from '@chakra-ui/react';
import React from 'react';

import config from 'configs/app';
import buildUrl from 'lib/api/buildUrl';
import * as cookies from 'lib/cookies';
import { Button } from 'toolkit/chakra/button';
import { toaster } from 'toolkit/chakra/toaster';
import { DAY, SECOND } from 'toolkit/utils/consts';
import { apos } from 'toolkit/utils/htmlEntities';
import ReCaptcha from 'ui/shared/reCaptcha/ReCaptcha';
import useReCaptcha from 'ui/shared/reCaptcha/useReCaptcha';

import AppErrorIcon from '../AppErrorIcon';
import AppErrorTitle from '../AppErrorTitle';

function formatTimeLeft(timeLeft: number) {
  const hours = Math.floor(timeLeft / 3600);
  const minutes = Math.floor((timeLeft % 3600) / 60);
  const seconds = timeLeft % 60;

  return `${ hours.toString().padStart(2, '0') }h ${ minutes.toString().padStart(2, '0') }m ${ seconds.toString().padStart(2, '0') }s`;
}

interface Props {
  bypassOptions?: string;
  reset?: string;
}

const AppErrorTooManyRequests = ({ bypassOptions, reset }: Props) => {

  const [ timeLeft, setTimeLeft ] = React.useState(reset ? Math.ceil(Number(reset) / SECOND) : undefined);

  const recaptcha = useReCaptcha();

  const handleSubmit = React.useCallback(async() => {
    try {
      const token = await recaptcha.executeAsync();

      if (!token) {
        throw new Error('ReCaptcha is not solved');
      }

      const url = buildUrl('general:api_v2_key', undefined, { in_header: true });

      const response = await fetch(url, {
        method: 'POST',
        body: JSON.stringify({ recaptcha_response: token }),
        headers: {
          'recaptcha-v2-response': token,
        },
      });

      if (!response.ok) {
        throw new Error(response.statusText);
      }

      const apiTempToken = response.headers.get('api-v2-temp-token');

      if (!apiTempToken) {
        throw new Error('API temp token is not found');
      }

      cookies.set(cookies.NAMES.API_TEMP_TOKEN, apiTempToken, {
        expires: reset ? Number(reset) / DAY : 1 / 24,
      });

      window.location.reload();

    } catch (error) {
      toaster.create({
        title: 'Error',
        description: 'Unable to get client key.',
        type: 'error',
      });
    }
  }, [ recaptcha, reset ]);

  React.useEffect(() => {
    if (reset === undefined) {
      return;
    }

    const interval = window.setInterval(() => {
      setTimeLeft((prev) => {
        if (prev && prev > 1) {
          return prev - 1;
        }

        window.clearInterval(interval);
        window.location.reload();

        return 0;
      });
    }, SECOND);

    return () => {
      window.clearInterval(interval);
    };
  }, [ reset ]);

  if (!config.services.reCaptchaV2.siteKey) {
    throw new Error('reCAPTCHA V2 site key is not set');
  }

  const text = (() => {
    if (timeLeft === undefined && bypassOptions === 'no_bypass') {
      return 'Rate limit exceeded.';
    }

    const timeLeftText = timeLeft !== undefined ? `wait ${ formatTimeLeft(timeLeft) } ` : '';
    const bypassText = bypassOptions !== 'no_bypass' ? `verify you${ apos }re human ` : '';
    const orText = timeLeft !== undefined && bypassOptions !== 'no_bypass' ? 'OR ' : '';

    return `Rate limit exceeded. Please ${ timeLeftText }${ orText }${ bypassText }before making another request.`;
  })();

  return (
    <>
      <AppErrorIcon statusCode={ 429 }/>
      <AppErrorTitle title="Too many requests"/>
      <Text color="text.secondary" mt={ 3 }>
        { text }
      </Text>
      <ReCaptcha { ...recaptcha }/>
      { bypassOptions !== 'no_bypass' && <Button onClick={ handleSubmit } disabled={ recaptcha.isInitError } mt={ 8 }>I'm not a robot</Button> }
    </>
  );
};

export default AppErrorTooManyRequests;
