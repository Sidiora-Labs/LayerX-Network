import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

import config from 'configs/app';

const validatorsFeature = config.features.validators;

const Validators = dynamic(() => {
  if (validatorsFeature.isEnabled && validatorsFeature.chainType === 'stability') {
    return import('ui/pages/ValidatorsStability');
  }

  if (validatorsFeature.isEnabled && validatorsFeature.chainType === 'blackfort') {
    return import('ui/pages/ValidatorsBlackfort');
  }

  if (validatorsFeature.isEnabled && validatorsFeature.chainType === 'zilliqa') {
    return import('ui/pages/ValidatorsZilliqa');
  }

  throw new Error('Validators feature is not enabled.');
}, { ssr: false });

const Page: NextPage = () => {
  return (
    <PageNextJs pathname="/validators">
      <Validators/>
    </PageNextJs>
  );
};

export default Page;

export { validators as getServerSideProps } from 'nextjs/getServerSideProps/main';
