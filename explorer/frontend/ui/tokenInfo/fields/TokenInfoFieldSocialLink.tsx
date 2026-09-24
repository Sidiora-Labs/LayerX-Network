import React from 'react';
import type { ControllerRenderProps } from 'react-hook-form';

import type { Fields, SocialLinkFields } from '../types';

import type { FieldProps } from 'toolkit/chakra/field';
import { FormFieldUrl } from 'toolkit/components/forms/fields/FormFieldUrl';
import type { IconName } from 'ui/shared/IconSvg';
import IconSvg from 'ui/shared/IconSvg';

interface Item {
  icon: IconName;
  label: string;
  color: string;
}
const SETTINGS: Record<keyof SocialLinkFields, Item> = {
  github: { label: 'GitHub', icon: 'social/github_filled', color: 'text.primary' },
  telegram: { label: 'Telegram', icon: 'social/telegram_filled', color: 'telegram' },
  linkedin: { label: 'LinkedIn', icon: 'social/linkedin_filled', color: 'linkedin' },
  discord: { label: 'Discord', icon: 'social/discord_filled', color: 'discord' },
  slack: { label: 'Slack', icon: 'social/slack_filled', color: 'slack' },
  twitter: { label: 'X (ex-Twitter)', icon: 'social/twitter_filled', color: 'text.primary' },
  opensea: { label: 'OpenSea', icon: 'social/opensea_filled', color: 'opensea' },
  facebook: { label: 'Facebook', icon: 'social/facebook_filled', color: 'facebook' },
  medium: { label: 'Medium', icon: 'social/medium_filled', color: 'text.primary' },
  reddit: { label: 'Reddit', icon: 'social/reddit_filled', color: 'reddit' },
};

interface Props {
  readOnly?: boolean;
  size?: FieldProps['size'];
  name: keyof SocialLinkFields;
}

const TokenInfoFieldSocialLink = ({ readOnly, size, name }: Props) => {

  const endElement = React.useCallback(({ field }: { field: ControllerRenderProps<Fields> }) => {
    return <IconSvg name={ SETTINGS[name].icon } boxSize="60px" px={ 4 } color={ field.value ? SETTINGS[name].color : '#718096' }/>;
  }, [ name ]);

  return (
    <FormFieldUrl<Fields>
      name={ name }
      placeholder={ SETTINGS[name].label }
      group={{
        endElement,
      }}
      readOnly={ readOnly }
      size={ size }
    />
  );
};

export default React.memo(TokenInfoFieldSocialLink);
