import type { NextPageContext } from 'next';
import NextErrorComponent from 'next/error';
import React from 'react';
import Rollbar from 'rollbar';

import type { Props as ServerSidePropsCommon } from 'nextjs/getServerSideProps/handlers';

import config from 'configs/app';
import * as cookies from 'lib/cookies';

const rollbarFeature = config.features.rollbar;
const rollbar = rollbarFeature.isEnabled ? new Rollbar({
  accessToken: rollbarFeature.clientToken,
  environment: rollbarFeature.environment,
  payload: {
    code_version: rollbarFeature.codeVersion,
    app_instance: rollbarFeature.instance,
  },
  maxItems: 10,
  captureUncaught: true,
  captureUnhandledRejections: true,
}) : undefined;

type Props = ServerSidePropsCommon & {
  statusCode: number;
};

const CustomErrorComponent = (props: Props) => {
  const colorModeCookie = cookies.getFromCookieString(props.cookies || '', cookies.NAMES.COLOR_MODE);
  return <NextErrorComponent statusCode={ props.statusCode } withDarkMode={ colorModeCookie === 'dark' }/>;
};

CustomErrorComponent.getInitialProps = async(context: NextPageContext) => {
  const { res, err, req } = context;

  const baseProps = await NextErrorComponent.getInitialProps(context); // Extract cookies from the request headers
  const statusCode = res?.statusCode ?? err?.statusCode;
  const cookies = req?.headers?.cookie || '';

  if (rollbar) {
    rollbar.error(err?.message ?? 'Unknown error', {
      cause: err?.cause,
      stack: err?.stack,
    });
  }

  return {
    ...baseProps,
    statusCode,
    cookies,
  };
};

export default CustomErrorComponent;
