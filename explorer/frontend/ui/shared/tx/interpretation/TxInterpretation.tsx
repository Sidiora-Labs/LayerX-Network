import type { BoxProps } from '@chakra-ui/react';
import { Box, chakra } from '@chakra-ui/react';
import BigNumber from 'bignumber.js';
import React from 'react';

import type { AddressParam } from 'types/api/addressParams';
import type {
  TxInterpretationSummary,
  TxInterpretationVariable,
  TxInterpretationVariableString,
} from 'types/api/txInterpretation';
import type { ClusterChainConfig } from 'types/multichain';

import { route } from 'nextjs-routes';

import config from 'configs/app';
import dayjs from 'lib/date/dayjs';
import * as mixpanel from 'lib/mixpanel/index';
import { currencyUnits } from 'lib/units';
import { Badge } from 'toolkit/chakra/badge';
import { useColorModeValue } from 'toolkit/chakra/color-mode';
import { Image } from 'toolkit/chakra/image';
import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { Tooltip } from 'toolkit/chakra/tooltip';
import { SECOND } from 'toolkit/utils/consts';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import EnsEntity from 'ui/shared/entities/ens/EnsEntity';
import TokenEntity from 'ui/shared/entities/token/TokenEntity';
import ChainIcon from 'ui/shared/externalChains/ChainIcon';
import getChainTooltipText from 'ui/shared/externalChains/getChainTooltipText';
import IconSvg from 'ui/shared/IconSvg';

import {
  extractVariables,
  getStringChunks,
  fillStringVariables,
  checkSummary,
  NATIVE_COIN_SYMBOL_VAR_NAME,
  WEI_VAR_NAME,
} from './utils';

const nameServicesFeature = config.features.nameServices;

interface Props extends BoxProps {
  summary?: TxInterpretationSummary;
  isLoading?: boolean;
  addressDataMap?: Record<string, AddressParam>;
  className?: string;
  isNoves?: boolean;
  chainData?: ClusterChainConfig;
};

type NonStringTxInterpretationVariable = Exclude<TxInterpretationVariable, TxInterpretationVariableString>;

const TxInterpretationElementByType = (
  { variable, addressDataMap }: { variable?: NonStringTxInterpretationVariable; addressDataMap?: Record<string, AddressParam> },
) => {
  const onAddressClick = React.useCallback(() => {
    mixpanel.logEvent(mixpanel.EventTypes.TX_INTERPRETATION_INTERACTION, { Type: 'Address click' });
  }, []);

  const onTokenClick = React.useCallback(() => {
    mixpanel.logEvent(mixpanel.EventTypes.TX_INTERPRETATION_INTERACTION, { Type: 'Token click' });
  }, []);

  const onDomainClick = React.useCallback(() => {
    mixpanel.logEvent(mixpanel.EventTypes.TX_INTERPRETATION_INTERACTION, { Type: 'Domain click' });
  }, []);

  if (!variable) {
    return null;
  }

  const { type, value } = variable;
  switch (type) {
    case 'address': {
      return (
        <chakra.span display="inline-block" verticalAlign="top" _notFirst={{ marginLeft: 1 }}>
          <AddressEntity
            address={ addressDataMap?.[value.hash] || value }
            icon={{ marginRight: 1 }}
            truncation="constant"
            onClick={ onAddressClick }
            whiteSpace="initial"
          />
        </chakra.span>
      );
    }
    case 'token':
      return (
        <chakra.span display="inline-block" verticalAlign="top" _notFirst={{ marginLeft: 1 }}>
          <TokenEntity
            token={ value }
            icon={{ marginRight: 1 }}
            onlySymbol
            noCopy
            width="fit-content"
            _notFirst={{ marginLeft: 1 }}
            mr={ 2 }
            whiteSpace="initial"
            onClick={ onTokenClick }
          />
        </chakra.span>
      );
    case 'domain': {
      if (nameServicesFeature.isEnabled && nameServicesFeature.ens.isEnabled) {
        return (
          <chakra.span display="inline-block" verticalAlign="top" _notFirst={{ marginLeft: 1 }}>
            <EnsEntity
              domain={ value }
              icon={{ marginRight: 1 }}
              width="fit-content"
              _notFirst={{ marginLeft: 1 }}
              whiteSpace="initial"
              onClick={ onDomainClick }
            />
          </chakra.span>
        );
      }
      return <chakra.span color="text.secondary" whiteSpace="pre">{ value + ' ' }</chakra.span>;
    }
    case 'currency': {
      let numberString = '';
      if (BigNumber(value).isLessThan(0.1)) {
        numberString = BigNumber(value).toPrecision(2);
      } else if (BigNumber(value).isLessThan(10000)) {
        numberString = BigNumber(value).dp(2).toFormat();
      } else if (BigNumber(value).isLessThan(1000000)) {
        numberString = BigNumber(value).dividedBy(1000).toFormat(2) + 'K';
      } else {
        numberString = BigNumber(value).dividedBy(1000000).toFormat(2) + 'M';
      }
      return <chakra.span>{ numberString + ' ' }</chakra.span>;
    }
    case 'timestamp': {
      return <chakra.span color="text.secondary" whiteSpace="pre">{ dayjs(Number(value) * SECOND).format('MMM DD YYYY') }</chakra.span>;
    }
    case 'external_link': {
      return <Link external href={ value.link }>{ value.name }</Link>;
    }
    case 'method': {
      return (
        <Badge
          colorPalette={ value === 'Multicall' ? 'teal' : 'gray' }
          truncated
          ml={ 1 }
          mr={ 2 }
          verticalAlign="text-top"
        >
          { value }
        </Badge>
      );
    }
    case 'dexTag': {
      const icon = value.app_icon || value.icon;
      const name = (() => {
        if (value.app_id && config.features.marketplace.isEnabled) {
          return (
            <Link
              href={ route({ pathname: '/apps/[id]', query: { id: value.app_id } }) }
            >
              { value.name }
            </Link>
          );
        }
        if (value.url) {
          return (
            <Link external href={ value.url }>
              { value.name }
            </Link>
          );
        }
        return value.name;
      })();

      return (
        <chakra.span display="inline-flex" alignItems="center" verticalAlign="top" _notFirst={{ marginLeft: 1 }} gap={ 1 } mr={ 2 }>
          { icon && <Image src={ icon } alt={ value.name } width={ 5 } height={ 5 }/> }
          { name }
        </chakra.span>
      );
    }
    case 'link': {
      return <Link external href={ value.url }>{ value.name }</Link>;
    }
  }
};

const TxInterpretation = ({ summary, isLoading, addressDataMap, className, chainData, isNoves, ...rest }: Props) => {
  const novesLogoUrl = useColorModeValue('/static/noves-logo.svg', '/static/noves-logo-dark.svg');
  if (!summary) {
    return null;
  }

  const template = summary.summary_template;
  const variables = summary.summary_template_variables;

  if (!checkSummary(template, variables)) {
    return null;
  }

  const intermediateResult = fillStringVariables(template, variables);

  const variablesNames = extractVariables(intermediateResult);
  const chunks = getStringChunks(intermediateResult);

  const tooltipContent = 'Transaction summary' + (chainData ? `\n${ getChainTooltipText(chainData) }` : '');

  return (
    <Skeleton loading={ isLoading } className={ className } fontWeight={ 500 } whiteSpace="pre-wrap" { ...rest }>
      <Tooltip content={ tooltipContent } contentProps={{ whiteSpace: 'pre-wrap' }}>
        <Box display="inline-flex" position="relative" mr={ chainData ? '14px' : 1 } verticalAlign="text-top">
          <IconSvg name="lightning" boxSize={ 5 } color="icon.primary"/>
          { chainData && (
            <ChainIcon
              data={ chainData }
              boxSize="18px"
              position="absolute"
              top="6px"
              left="12px"
              borderRadius="full"
              borderWidth="1px"
              borderStyle="solid"
              borderColor="bg.primary"
              backgroundColor="bg.primary"
              noTooltip
            />
          ) }
        </Box>
      </Tooltip>
      { chunks.map((chunk, index) => {
        let content = null;
        if (variablesNames[index] === NATIVE_COIN_SYMBOL_VAR_NAME) {
          content = <chakra.span>{ currencyUnits.ether + ' ' }</chakra.span>;
        } else if (variablesNames[index] === WEI_VAR_NAME) {
          content = <chakra.span>{ currencyUnits.wei + ' ' }</chakra.span>;
        } else {
          content = (
            <TxInterpretationElementByType
              variable={ variables[variablesNames[index]] as NonStringTxInterpretationVariable }
              addressDataMap={ addressDataMap }
            />
          );
        }
        return (
          <chakra.span key={ chunk + index }>
            <chakra.span color="text.secondary">{ chunk.trim() + (chunk.trim() && variablesNames[index] ? ' ' : '') }</chakra.span>
            { index < variablesNames.length && content }
          </chakra.span>
        );
      }) }
      { isNoves && (
        <Tooltip content="Human readable transaction provided by Noves.fi">
          <Badge ml={ 2 } verticalAlign="unset" transform="translateY(-2px)">
            by
            <Image src={ novesLogoUrl } alt="Noves logo" h="12px" ml={ 1.5 } display="inline"/>
          </Badge>
        </Tooltip>
      ) }
    </Skeleton>
  );
};

export default chakra(TxInterpretation);
