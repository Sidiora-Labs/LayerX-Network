import React from 'react';

import config from 'configs/app';
import { useAppContext } from 'lib/contexts/app';
import * as cookies from 'lib/cookies';
import { Switch } from 'toolkit/chakra/switch';

const SettingsScamTokens = () => {
  const { cookies: appCookies } = useAppContext();

  const initialValue = cookies.get(cookies.NAMES.SHOW_SCAM_TOKENS, appCookies);

  const [ isChecked, setIsChecked ] = React.useState(initialValue !== 'true');

  const handleChange = React.useCallback(() => {
    setIsChecked(prev => {
      const nextValue = !prev;
      cookies.set(cookies.NAMES.SHOW_SCAM_TOKENS, nextValue ? 'false' : 'true');
      return nextValue;
    });
    window.location.reload();
  }, []);

  if (!config.UI.views.token.hideScamTokensEnabled) {
    return null;
  }

  return (
    <Switch
      id="scam-tokens"
      checked={ isChecked }
      onChange={ handleChange }
      direction="rtl"
      justifyContent="space-between"
      w="100%"
      minH="34px"
    >
      Hide scam tokens
    </Switch>
  );
};

export default React.memo(SettingsScamTokens);
