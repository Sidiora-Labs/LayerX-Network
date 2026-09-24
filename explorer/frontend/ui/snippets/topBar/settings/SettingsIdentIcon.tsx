import { Box, Flex } from '@chakra-ui/react';
import React from 'react';

import config from 'configs/app';
import * as cookies from 'lib/cookies';
import { IDENTICONS } from 'lib/settings/identIcon';

import SettingsSample from './SettingsSample';

const SettingsIdentIcon = () => {
  const [ activeId, setActiveId ] = React.useState<string>();

  React.useEffect(() => {
    const initialId = cookies.get(cookies.NAMES.ADDRESS_IDENTICON_TYPE) || config.UI.views.address.identiconType;
    setActiveId(initialId);
  }, []);

  const handleSelect = React.useCallback((event: React.MouseEvent<HTMLDivElement>) => {
    event.stopPropagation();

    const id = event.currentTarget.getAttribute('data-value');

    if (!id) {
      return;
    }

    cookies.set(cookies.NAMES.ADDRESS_IDENTICON_TYPE, id);
    window.location.reload();
  }, []);

  const activeIdenticon = IDENTICONS.find((identicon) => identicon.id === activeId);

  return (
    <div>
      <Box fontWeight={ 600 }>Address settings</Box>
      <Box color="text.secondary" mt={ 1 } mb={ 2 }>{ activeIdenticon?.label }</Box>
      <Flex>
        { IDENTICONS.map((identicon) => (
          <SettingsSample
            key={ identicon.id }
            label={ identicon.label }
            value={ identicon.id }
            bg={ identicon.sampleBg }
            isActive={ identicon.id === activeId }
            onClick={ handleSelect }
          />
        )) }
      </Flex>
    </div>
  );
};

export default React.memo(SettingsIdentIcon);
