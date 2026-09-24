import type { FlexProps } from '@chakra-ui/react';
import { Flex } from '@chakra-ui/react';
import React from 'react';

import config from 'configs/app';
import AlertWithExternalHtml from 'ui/shared/alerts/AlertWithExternalHtml';

import IndexingBlocksAlert from './alerts/IndexingBlocksAlert';

const maintenanceAlertHtml = config.UI.maintenanceAlert.message || '';

const HeaderAlert = (props: FlexProps) => {
  return (
    <Flex flexDir="column" rowGap={ 1 } mb={{ base: 6, lg: 3 }} _empty={{ display: 'none' }} { ...props }>
      { maintenanceAlertHtml && <AlertWithExternalHtml html={ maintenanceAlertHtml } status="info" showIcon/> }
      <IndexingBlocksAlert/>
    </Flex>
  );
};

export default React.memo(HeaderAlert);
