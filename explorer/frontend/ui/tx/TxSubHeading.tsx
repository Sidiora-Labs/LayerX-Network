import { Box, Flex } from '@chakra-ui/react';
import React from 'react';

import type { AddressParam } from 'types/api/addressParams';

import config from 'configs/app';
import useApiQuery from 'lib/api/useApiQuery';
import { useMultichainContext } from 'lib/contexts/multichain';
import { NOVES_TRANSLATE } from 'stubs/noves/NovesTranslate';
import { TX_INTERPRETATION } from 'stubs/txInterpretation';
import { Link } from 'toolkit/chakra/link';
import AccountActionsMenu from 'ui/shared/AccountActionsMenu/AccountActionsMenu';
import AppActionButton from 'ui/shared/AppActionButton/AppActionButton';
import useAppActionData from 'ui/shared/AppActionButton/useAppActionData';
import { TX_ACTIONS_BLOCK_ID } from 'ui/shared/DetailedInfo/DetailedInfoActionsWrapper';
import TxEntity from 'ui/shared/entities/tx/TxEntity';
import NetworkExplorers from 'ui/shared/NetworkExplorers';
import TxInterpretation from 'ui/shared/tx/interpretation/TxInterpretation';

import { createNovesSummaryObject } from './assetFlows/utils/createNovesSummaryObject';
import type { TxQuery } from './useTxQuery';

type Props = {
  hash: string;
  hasTag: boolean;
  txQuery: TxQuery;
};

const TxSubHeading = ({ hash, hasTag, txQuery }: Props) => {
  const multichainContext = useMultichainContext();
  const feature = multichainContext?.chain?.app_config.features.txInterpretation || config.features.txInterpretation;

  const hasInterpretationFeature = feature.isEnabled;
  const isNovesInterpretation = hasInterpretationFeature && feature.provider === 'noves';

  const appActionData = useAppActionData(txQuery.data?.to?.hash, !txQuery.isPlaceholderData);

  const txInterpretationQuery = useApiQuery('general:tx_interpretation', {
    pathParams: { hash },
    queryOptions: {
      enabled: Boolean(hash) && (hasInterpretationFeature && !isNovesInterpretation),
      placeholderData: TX_INTERPRETATION,
    },
  });

  const novesInterpretationQuery = useApiQuery('general:noves_transaction', {
    pathParams: { hash },
    queryOptions: {
      enabled: Boolean(hash) && isNovesInterpretation,
      placeholderData: NOVES_TRANSLATE,
    },
  });

  const hasNovesInterpretation = isNovesInterpretation &&
    (novesInterpretationQuery.isPlaceholderData || Boolean(novesInterpretationQuery.data?.classificationData.description));

  const hasInternalInterpretation = (hasInterpretationFeature && !isNovesInterpretation) &&
  (txInterpretationQuery.isPlaceholderData || Boolean(txInterpretationQuery.data?.data.summaries.length));

  const hasViewAllInterpretationsLink =
    !txInterpretationQuery.isPlaceholderData && txInterpretationQuery.data?.data.summaries && txInterpretationQuery.data?.data.summaries.length > 1;

  const addressDataMap: Record<string, AddressParam> = {};
  [ txQuery.data?.from, txQuery.data?.to ]
    .filter((data): data is AddressParam => Boolean(data && data.hash))
    .forEach(data => {
      addressDataMap[data.hash] = data;
    });

  const content = (() => {
    if (hasNovesInterpretation && novesInterpretationQuery.data) {
      const novesSummary = createNovesSummaryObject(novesInterpretationQuery.data);
      return (
        <TxInterpretation
          summary={ novesSummary }
          isLoading={ novesInterpretationQuery.isPlaceholderData || txQuery.isPlaceholderData }
          addressDataMap={ addressDataMap }
          fontSize="lg"
          mr={{ base: 0, lg: 2 }}
          isNoves
          chainData={ multichainContext?.chain }
        />
      );
    } else if (hasInternalInterpretation) {
      return (
        <Flex mr={{ base: 0, lg: 2 }} flexWrap="wrap" alignItems="center">
          <TxInterpretation
            summary={ txInterpretationQuery.data?.data.summaries[0] }
            isLoading={ txInterpretationQuery.isPlaceholderData || txQuery.isPlaceholderData }
            addressDataMap={ addressDataMap }
            fontSize="lg"
            mr={ hasViewAllInterpretationsLink ? 3 : 0 }
            chainData={ multichainContext?.chain }
          />
          { hasViewAllInterpretationsLink &&
          <Link href={ `#${ TX_ACTIONS_BLOCK_ID }` }>View all</Link> }
        </Flex>
      );
    } else if (hasInterpretationFeature && txQuery.data?.method && txQuery.data?.from && txQuery.data?.to && !txQuery.isPlaceholderData) {
      return (
        <TxInterpretation
          summary={{
            summary_template: `{sender_hash} ${ txQuery.data.status === 'error' ? 'failed to call' : 'called' } {method} on {receiver_hash}`,
            summary_template_variables: {
              sender_hash: {
                type: 'address',
                value: txQuery.data.from,
              },
              method: {
                type: 'method',
                value: txQuery.data.method,
              },
              receiver_hash: {
                type: 'address',
                value: txQuery.data.to,
              },
            },
          }}
          fontSize="lg"
          mr={{ base: 0, lg: 2 }}
          chainData={ multichainContext?.chain }
        />
      );
    } else {
      return <TxEntity hash={ hash } noLink variant="subheading" mr={{ base: 0, lg: 2 }} chain={ multichainContext?.chain }/>;
    }
  })();

  const isLoading =
    txQuery.isPlaceholderData ||
    (hasNovesInterpretation && novesInterpretationQuery.isPlaceholderData) ||
    (hasInternalInterpretation && txInterpretationQuery.isPlaceholderData);

  return (
    <Box display={{ base: 'block', lg: 'flex' }} alignItems="center" w="100%">
      { content }
      <Flex
        alignItems="center"
        justifyContent={{ base: 'start', lg: 'space-between' }}
        flexGrow={ 1 }
        gap={ 3 }
        mt={{ base: 3, lg: 0 }}
      >
        { !hasTag && <AccountActionsMenu isLoading={ isLoading }/> }
        { appActionData && (
          <AppActionButton data={ appActionData } txHash={ hash } source="Txn"/>
        ) }
        <NetworkExplorers type="tx" pathParam={ hash } ml="auto"/>
      </Flex>
    </Box>
  );
};

export default TxSubHeading;
