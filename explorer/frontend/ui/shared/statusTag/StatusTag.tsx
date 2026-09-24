import React from 'react';

import capitalizeFirstLetter from 'lib/capitalizeFirstLetter';
import type { BadgeProps } from 'toolkit/chakra/badge';
import { Badge } from 'toolkit/chakra/badge';
import { Tooltip } from 'toolkit/chakra/tooltip';
import type { IconName } from 'ui/shared/IconSvg';
import IconSvg from 'ui/shared/IconSvg';

export type StatusTagType = 'ok' | 'error' | 'pending';

export interface Props extends BadgeProps {
  type: 'ok' | 'error' | 'pending';
  text?: string;
  errorText?: string | null;
  mode?: 'compact' | 'full';
}

const StatusTag = ({ type, text, errorText, mode = 'full', ...rest }: Props) => {
  let icon: IconName;
  let colorPalette: BadgeProps['colorPalette'];

  switch (type) {
    case 'ok':
      icon = 'status/success';
      colorPalette = 'green';
      break;
    case 'error':
      icon = 'status/error';
      colorPalette = 'red';
      break;
    case 'pending':
      icon = 'status/pending';
      colorPalette = 'gray';
      break;
  }

  const iconElement = <IconSvg name={ icon } boxSize={ 2.5 } display={ text ? 'inline-block' : 'block' }/>;
  const capitalizedText = text ? capitalizeFirstLetter(text) : undefined;

  if (mode === 'compact') {
    const tooltipContent = errorText || capitalizedText;
    return (
      <Tooltip content={ tooltipContent } disabled={ !tooltipContent }>
        <Badge colorPalette={ colorPalette } startElement={ iconElement } px="7px" { ...rest }/>
      </Tooltip>
    );
  }

  if (!text) {
    return (
      <Badge colorPalette={ colorPalette } { ...rest }>
        { iconElement }
      </Badge>
    );
  }

  return (
    <Tooltip content={ errorText } disabled={ !errorText }>
      <Badge colorPalette={ colorPalette } startElement={ iconElement } { ...rest }>
        { capitalizedText }
      </Badge>
    </Tooltip>
  );
};

export default StatusTag;
