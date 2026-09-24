import { GridItem, Text, Box } from '@chakra-ui/react';
import BigNumber from 'bignumber.js';
import { capitalize } from 'es-toolkit';
import { useRouter } from 'next/router';
import React from 'react';

import { ZKSYNC_L2_TX_BATCH_STATUSES } from 'types/api/zkSyncL2';

import { route, routeParams } from 'nextjs/routes';

import config from 'configs/app';
import getBlockReward from 'lib/block/getBlockReward';
import { useMultichainContext } from 'lib/contexts/multichain';
import getNetworkValidatorTitle from 'lib/networks/getNetworkValidatorTitle';
import * as arbitrum from 'lib/rollups/arbitrum';
import { formatZkSyncL2TxnBatchStatus, layerLabels } from 'lib/rollups/utils';
import getQueryParamString from 'lib/router/getQueryParamString';
import { CollapsibleDetails } from 'toolkit/chakra/collapsible';
import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { Tooltip } from 'toolkit/chakra/tooltip';
import { ZERO } from 'toolkit/utils/consts';
import { space } from 'toolkit/utils/htmlEntities';
import OptimisticL2TxnBatchDA from 'ui/shared/batch/OptimisticL2TxnBatchDA';
import BlockGasUsed from 'ui/shared/block/BlockGasUsed';
import CopyToClipboard from 'ui/shared/CopyToClipboard';
import * as DetailedInfo from 'ui/shared/DetailedInfo/DetailedInfo';
import DetailedInfoTimestamp from 'ui/shared/DetailedInfo/DetailedInfoTimestamp';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import BatchEntityL2 from 'ui/shared/entities/block/BatchEntityL2';
import BlockEntityL1 from 'ui/shared/entities/block/BlockEntityL1';
import TxEntityL1 from 'ui/shared/entities/tx/TxEntityL1';
import HashStringShortenDynamic from 'ui/shared/HashStringShortenDynamic';
import IconSvg from 'ui/shared/IconSvg';
import PrevNext from 'ui/shared/PrevNext';
import RawDataSnippet from 'ui/shared/RawDataSnippet';
import StatusTag from 'ui/shared/statusTag/StatusTag';
import Utilization from 'ui/shared/Utilization/Utilization';
import GasPriceValue from 'ui/shared/value/GasPriceValue';
import NativeCoinValue from 'ui/shared/value/NativeCoinValue';
import { WEI } from 'ui/shared/value/utils';
import VerificationSteps from 'ui/shared/verificationSteps/VerificationSteps';
import ZkSyncL2TxnBatchHashesInfo from 'ui/txnBatches/zkSyncL2/ZkSyncL2TxnBatchHashesInfo';

import BlockDetailsBaseFeeCelo from './details/BlockDetailsBaseFeeCelo';
import BlockDetailsBlobInfo from './details/BlockDetailsBlobInfo';
import BlockDetailsZilliqaQuorumCertificate from './details/BlockDetailsZilliqaQuorumCertificate';
import type { BlockQuery } from './useBlockQuery';

const zkSyncVerificationSteps = ZKSYNC_L2_TX_BATCH_STATUSES.map(formatZkSyncL2TxnBatchStatus);

interface Props {
  query: BlockQuery;
}

const rollupFeature = config.features.rollup;

const BlockDetails = ({ query }: Props) => {
  const router = useRouter();
  const heightOrHash = getQueryParamString(router.query.height_or_hash);
  const multichainContext = useMultichainContext();

  const { data, isPlaceholderData } = query;

  const handlePrevNextClick = React.useCallback((direction: 'prev' | 'next') => {
    if (!data) {
      return;
    }

    const increment = direction === 'next' ? +1 : -1;
    const nextId = String(data.height + increment);

    router.push(routeParams({ pathname: '/block/[height_or_hash]', query: { height_or_hash: nextId } }, { chain: multichainContext?.chain }));
  }, [ data, multichainContext, router ]);

  if (!data) {
    return null;
  }

  const { totalReward, staticReward, burntFees, txFees } = getBlockReward(data);

  const validatorTitle = getNetworkValidatorTitle();

  const rewardBreakDown = (() => {
    if (rollupFeature.isEnabled || totalReward.isEqualTo(ZERO) || txFees.isEqualTo(ZERO) || burntFees.isEqualTo(ZERO)) {
      return null;
    }

    if (isPlaceholderData) {
      return <Skeleton loading w="525px" h="20px"/>;
    }

    return (
      <Text color="text.secondary" whiteSpace="break-spaces">
        <Tooltip content="Static block reward">
          <span>{ staticReward.dividedBy(WEI).toFixed() }</span>
        </Tooltip>
        { !txFees.isEqualTo(ZERO) && (
          <>
            { space }+{ space }
            <Tooltip content="Txn fees">
              <span>{ txFees.dividedBy(WEI).toFixed() }</span>
            </Tooltip>
          </>
        ) }
        { !burntFees.isEqualTo(ZERO) && (
          <>
            { space }-{ space }
            <Tooltip content="Burnt fees">
              <span>{ burntFees.dividedBy(WEI).toFixed() }</span>
            </Tooltip>
          </>
        ) }
      </Text>
    );
  })();

  const txsNum = (() => {
    const blockTxsNum = (
      <Link href={ route({ pathname: '/block/[height_or_hash]', query: { height_or_hash: heightOrHash, tab: 'txs' } }, multichainContext) }>
        { data.transactions_count } txn{ data.transactions_count === 1 ? '' : 's' }
      </Link>
    );

    const blockBlobTxsNum = (config.features.dataAvailability.isEnabled && data.blob_transactions_count) ? (
      <>
        <span> including </span>
        <Link href={ route({ pathname: '/block/[height_or_hash]', query: { height_or_hash: heightOrHash, tab: 'blob_txs' } }, multichainContext) }>
          { data.blob_transactions_count } blob txn{ data.blob_transactions_count === 1 ? '' : 's' }
        </Link>
      </>
    ) : null;

    return (
      <>
        { blockTxsNum }
        { blockBlobTxsNum }
        <span> in this block</span>
      </>
    );
  })();

  const blockTypeLabel = (() => {
    switch (data.type) {
      case 'reorg':
        return 'Reorg';
      case 'uncle':
        return 'Uncle';
      default:
        return 'Block';
    }
  })();

  return (
    <DetailedInfo.Container templateColumns={{ base: 'minmax(0, 1fr)', lg: 'minmax(min-content, 200px) minmax(0, 1fr)' }} >
      <DetailedInfo.ItemLabel
        hint="The block height of a particular block is defined as the number of blocks preceding it in the blockchain"
        isLoading={ isPlaceholderData }
      >
        { blockTypeLabel } height
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <Skeleton loading={ isPlaceholderData }>
          { data.height }
        </Skeleton>
        { data.height === 0 && <Text whiteSpace="pre"> - Genesis Block</Text> }
        <PrevNext
          ml={ 6 }
          onClick={ handlePrevNextClick }
          prevLabel="View previous block"
          nextLabel="View next block"
          isPrevDisabled={ data.height === 0 }
          isLoading={ isPlaceholderData }
        />
      </DetailedInfo.ItemValue>

      { rollupFeature.isEnabled && rollupFeature.type === 'arbitrum' && data.arbitrum && (
        <>
          <DetailedInfo.ItemLabel
            hint={ `The most recent ${ layerLabels.parent } block height as of this ${ layerLabels.current } block` }
            isLoading={ isPlaceholderData }
          >
            { layerLabels.parent } block height
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue>
            <BlockEntityL1 isLoading={ isPlaceholderData } number={ data.arbitrum.l1_block_number }/>
          </DetailedInfo.ItemValue>
        </>
      ) }

      { rollupFeature.isEnabled && rollupFeature.type === 'arbitrum' && data.arbitrum && !config.UI.views.block.hiddenFields?.batch && (
        <>
          <DetailedInfo.ItemLabel
            hint="Batch number"
            isLoading={ isPlaceholderData }
          >
            Batch
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue>
            { data.arbitrum.batch_number ?
              <BatchEntityL2 isLoading={ isPlaceholderData } number={ data.arbitrum.batch_number }/> :
              <Skeleton loading={ isPlaceholderData }>Pending</Skeleton> }
          </DetailedInfo.ItemValue>
        </>
      ) }

      { rollupFeature.isEnabled && rollupFeature.type === 'optimistic' && data.optimism && !config.UI.views.block.hiddenFields?.batch && (
        <>
          <DetailedInfo.ItemLabel
            hint="Batch number"
            isLoading={ isPlaceholderData }
          >
            Batch
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue columnGap={ 3 }>
            { data.optimism.number ?
              <BatchEntityL2 isLoading={ isPlaceholderData } number={ data.optimism.number }/> :
              <Skeleton loading={ isPlaceholderData }>Pending</Skeleton> }
            { data.optimism.batch_data_container && (
              <OptimisticL2TxnBatchDA
                container={ data.optimism.batch_data_container }
                isLoading={ isPlaceholderData }
              />
            ) }
          </DetailedInfo.ItemValue>
        </>
      ) }

      { typeof data.size === 'number' && (
        <>
          <DetailedInfo.ItemLabel
            hint="Size of the block in bytes"
            isLoading={ isPlaceholderData }
          >
            Size
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue>
            <Skeleton loading={ isPlaceholderData }>
              { data.size.toLocaleString() }
            </Skeleton>
          </DetailedInfo.ItemValue>
        </>
      ) }

      <DetailedInfo.ItemLabel
        hint="Date & time at which block was produced."
        isLoading={ isPlaceholderData }
      >
        Timestamp
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <DetailedInfoTimestamp timestamp={ data.timestamp } isLoading={ isPlaceholderData }/>
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel
        hint="The number of transactions in the block"
        isLoading={ isPlaceholderData }
      >
        Transactions
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <Skeleton loading={ isPlaceholderData }>
          { txsNum }
        </Skeleton>
      </DetailedInfo.ItemValue>

      { config.features.beaconChain.isEnabled && Boolean(data.withdrawals_count) && (
        <>
          <DetailedInfo.ItemLabel
            hint="The number of beacon withdrawals in the block"
            isLoading={ isPlaceholderData }
          >
            Withdrawals
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue>
            <Skeleton loading={ isPlaceholderData }>
              <Link href={ route({ pathname: '/block/[height_or_hash]', query: { height_or_hash: heightOrHash, tab: 'withdrawals' } }, multichainContext) }>
                { data.withdrawals_count } withdrawal{ data.withdrawals_count === 1 ? '' : 's' }
              </Link>
            </Skeleton>
          </DetailedInfo.ItemValue>
        </>
      ) }

      { rollupFeature.isEnabled && rollupFeature.type === 'zkSync' && data.zksync && !config.UI.views.block.hiddenFields?.batch && (
        <>
          <DetailedInfo.ItemLabel
            hint="Batch number"
            isLoading={ isPlaceholderData }
          >
            Batch
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue>
            { data.zksync.batch_number ?
              <BatchEntityL2 isLoading={ isPlaceholderData } number={ data.zksync.batch_number }/> :
              <Skeleton loading={ isPlaceholderData }>Pending</Skeleton> }
          </DetailedInfo.ItemValue>
        </>
      ) }
      { !config.UI.views.block.hiddenFields?.L1_status && rollupFeature.isEnabled &&
        ((rollupFeature.type === 'zkSync' && data.zksync) || (rollupFeature.type === 'arbitrum' && data.arbitrum)) &&
      (
        <>
          <DetailedInfo.ItemLabel
            hint="Status is the short interpretation of the batch lifecycle"
            isLoading={ isPlaceholderData }
          >
            Status
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue>
            { rollupFeature.type === 'zkSync' && data.zksync && (
              <VerificationSteps
                steps={ zkSyncVerificationSteps }
                currentStep={ formatZkSyncL2TxnBatchStatus(data.zksync.status) }
                isLoading={ isPlaceholderData }
              />
            ) }
            { rollupFeature.type === 'arbitrum' && data.arbitrum && (
              <VerificationSteps
                steps={ arbitrum.verificationSteps }
                currentStep={ arbitrum.VERIFICATION_STEPS_MAP[data.arbitrum.status] }
                currentStepPending={ arbitrum.getVerificationStepStatus(data.arbitrum) === 'pending' }
                isLoading={ isPlaceholderData }
              />
            ) }
          </DetailedInfo.ItemValue>
        </>
      ) }

      { !config.UI.views.block.hiddenFields?.miner && (
        <>
          <DetailedInfo.ItemLabel
            hint="A block producer who successfully included the block onto the blockchain"
            isLoading={ isPlaceholderData }
          >
            { capitalize(validatorTitle) }
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue>
            <AddressEntity
              address={ data.miner }
              isLoading={ isPlaceholderData }
            />
          </DetailedInfo.ItemValue>
        </>
      ) }

      { rollupFeature.isEnabled && rollupFeature.type === 'arbitrum' &&
        (data.arbitrum?.commitment_transaction.hash || data.arbitrum?.confirmation_transaction.hash) &&
      (
        <>
          <DetailedInfo.ItemDivider/>
          { data.arbitrum?.commitment_transaction.hash && (
            <>
              <DetailedInfo.ItemLabel
                hint={ `${ layerLabels.parent } transaction containing this batch commitment` }
                isLoading={ isPlaceholderData }
              >
                Commitment tx
              </DetailedInfo.ItemLabel>
              <DetailedInfo.ItemValue>
                <TxEntityL1 hash={ data.arbitrum?.commitment_transaction.hash } isLoading={ isPlaceholderData }/>
                { data.arbitrum?.commitment_transaction.status === 'finalized' && <StatusTag type="ok" text="Finalized" ml={ 2 }/> }
              </DetailedInfo.ItemValue>
            </>
          ) }
          { data.arbitrum?.confirmation_transaction.hash && (
            <>
              <DetailedInfo.ItemLabel
                hint={ `${ layerLabels.parent } transaction containing confirmation of this batch` }
                isLoading={ isPlaceholderData }
              >
                Confirmation tx
              </DetailedInfo.ItemLabel>
              <DetailedInfo.ItemValue>
                <TxEntityL1 hash={ data.arbitrum?.confirmation_transaction.hash } isLoading={ isPlaceholderData }/>
                { data.arbitrum?.commitment_transaction.status === 'finalized' && <StatusTag type="ok" text="Finalized" ml={ 2 }/> }
              </DetailedInfo.ItemValue>
            </>
          ) }
        </>
      ) }

      { !rollupFeature.isEnabled && !totalReward.isEqualTo(ZERO) && !config.UI.views.block.hiddenFields?.total_reward && (
        <>
          <DetailedInfo.ItemLabel
            hint={
              `For each block, the ${ validatorTitle } is rewarded with a finite amount of ${ config.chain.currency.symbol || 'native token' } 
          on top of the fees paid for all transactions in the block`
            }
            isLoading={ isPlaceholderData }
          >
            Block reward
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue columnGap={ 1 } multiRow>
            <NativeCoinValue amount={ totalReward.toString() } accuracy={ 0 } loading={ isPlaceholderData }/>
            { rewardBreakDown }
          </DetailedInfo.ItemValue>
        </>
      ) }

      { data.rewards
        ?.filter(({ type }) => type !== 'Validator Reward' && type !== 'Miner Reward')
        .map(({ type, reward }) => (
          <React.Fragment key={ type }>
            <DetailedInfo.ItemLabel
              hint={ `Amount of distributed reward. ${ capitalize(validatorTitle) }s receive a static block reward + Tx fees + uncle fees` }
            >
              { type }
            </DetailedInfo.ItemLabel>
            <DetailedInfo.ItemValue>
              <NativeCoinValue amount={ reward.toString() } accuracy={ 0 }/>
            </DetailedInfo.ItemValue>
          </React.Fragment>
        ))
      }

      { typeof data.zilliqa?.view === 'number' && (
        <>
          <DetailedInfo.ItemLabel
            hint="The iteration of the consensus round in which the block was proposed"
            isLoading={ isPlaceholderData }
          >
            View
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue>
            <Skeleton loading={ isPlaceholderData }>
              { data.zilliqa.view }
            </Skeleton>
          </DetailedInfo.ItemValue>
        </>
      ) }

      <DetailedInfo.ItemDivider/>

      { data.celo?.base_fee && <BlockDetailsBaseFeeCelo data={ data.celo.base_fee }/> }

      <DetailedInfo.ItemLabel
        hint="The total gas amount used in the block and its percentage of gas filled in the block"
        isLoading={ isPlaceholderData }
      >
        Gas used
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <Skeleton loading={ isPlaceholderData }>
          { BigNumber(data.gas_used || 0).toFormat() }
        </Skeleton>
        <BlockGasUsed
          gasUsed={ data.gas_used || undefined }
          gasLimit={ data.gas_limit }
          isLoading={ isPlaceholderData }
          ml={ 4 }
          gasTarget={ data.gas_target_percentage || undefined }
        />
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel
        hint="Total gas limit provided by all transactions in the block"
        isLoading={ isPlaceholderData }
      >
        Gas limit
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <Skeleton loading={ isPlaceholderData }>
          { BigNumber(data.gas_limit).toFormat() }
        </Skeleton>
      </DetailedInfo.ItemValue>

      { data.minimum_gas_price && (
        <>
          <DetailedInfo.ItemLabel
            hint="The minimum gas price a transaction should have in order to be included in this block"
            isLoading={ isPlaceholderData }
          >
            Minimum gas price
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue>
            <NativeCoinValue amount={ data.minimum_gas_price } units="gwei" loading={ isPlaceholderData }/>
          </DetailedInfo.ItemValue>
        </>
      ) }

      { data.base_fee_per_gas && (
        <>
          <DetailedInfo.ItemLabel
            hint="Minimum fee required per unit of gas. Fee adjusts based on network congestion"
            isLoading={ isPlaceholderData }
          >
            Base fee per gas
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue multiRow>
            <GasPriceValue
              amount={ data.base_fee_per_gas }
              loading={ isPlaceholderData }
            />
          </DetailedInfo.ItemValue>
        </>
      ) }

      { !config.UI.views.block.hiddenFields?.burnt_fees && !burntFees.isEqualTo(ZERO) && (
        <>
          <DetailedInfo.ItemLabel
            hint={
              `Amount of ${ config.chain.currency.symbol || 'native token' } burned from transactions included in the block. 
              Equals Block Base Fee per Gas * Gas Used`
            }
            isLoading={ isPlaceholderData }
          >
            Burnt fees
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue multiRow>
            <NativeCoinValue
              amount={ burntFees.toString() }
              accuracy={ 0 }
              loading={ isPlaceholderData }
              startElement={ <IconSvg name="flame" boxSize={ 5 } mr={{ base: 1, lg: 2 }} color="icon.primary" isLoading={ isPlaceholderData }/> }
              mr={ 4 }
            />
            { !txFees.isEqualTo(ZERO) && (
              <Tooltip content="Burnt fees / Txn fees * 100%">
                <Utilization
                  value={ burntFees.dividedBy(txFees).toNumber() }
                  isLoading={ isPlaceholderData }
                />
              </Tooltip>
            ) }
          </DetailedInfo.ItemValue>
        </>
      ) }

      { data.priority_fee !== null && BigNumber(data.priority_fee).gt(ZERO) && (
        <>
          <DetailedInfo.ItemLabel
            hint="User-defined tips sent to validator for transaction priority/inclusion"
            isLoading={ isPlaceholderData }
          >
            Priority fee / Tip
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue>
            <NativeCoinValue amount={ data.priority_fee.toString() } accuracy={ 0 } loading={ isPlaceholderData }/>
          </DetailedInfo.ItemValue>
        </>
      ) }

      { /* ADDITIONAL INFO */ }
      <CollapsibleDetails loading={ isPlaceholderData } mt={ 6 } gridColumn={{ base: undefined, lg: '1 / 3' }}>
        <GridItem colSpan={{ base: undefined, lg: 2 }} mt={{ base: 1, lg: 4 }}/>

        { rollupFeature.isEnabled && rollupFeature.type === 'zkSync' && data.zksync &&
              <ZkSyncL2TxnBatchHashesInfo data={ data.zksync } isLoading={ isPlaceholderData }/> }

        { !isPlaceholderData && <BlockDetailsBlobInfo data={ data }/> }

        { data.bitcoin_merged_mining_header && (
          <>
            <DetailedInfo.ItemLabel
              hint="Merged-mining field: Bitcoin header"
            >
              Bitcoin merged mining header
            </DetailedInfo.ItemLabel>
            <DetailedInfo.ItemValue
              flexWrap="nowrap"
              alignSelf="flex-start"
            >
              <Box whiteSpace="nowrap" overflow="hidden">
                <HashStringShortenDynamic hash={ data.bitcoin_merged_mining_header }/>
              </Box>
              <CopyToClipboard text={ data.bitcoin_merged_mining_header }/>
            </DetailedInfo.ItemValue>
          </>
        ) }

        { data.bitcoin_merged_mining_coinbase_transaction && (
          <>
            <DetailedInfo.ItemLabel
              hint="Merged-mining field: Coinbase transaction"
            >
              Bitcoin merged mining coinbase transaction
            </DetailedInfo.ItemLabel>
            <DetailedInfo.ItemValue>
              <RawDataSnippet
                data={ data.bitcoin_merged_mining_coinbase_transaction }
                isLoading={ isPlaceholderData }
                showCopy={ false }
                textareaMaxHeight="100px"
              />
            </DetailedInfo.ItemValue>
          </>
        ) }

        { data.bitcoin_merged_mining_merkle_proof && (
          <>
            <DetailedInfo.ItemLabel
              hint="Merged-mining field: Merkle proof"
            >
              Bitcoin merged mining Merkle proof
            </DetailedInfo.ItemLabel>
            <DetailedInfo.ItemValue>
              <RawDataSnippet
                data={ data.bitcoin_merged_mining_merkle_proof }
                isLoading={ isPlaceholderData }
                showCopy={ false }
                textareaMaxHeight="100px"
              />
            </DetailedInfo.ItemValue>
          </>
        ) }

        { data.hash_for_merged_mining && (
          <>
            <DetailedInfo.ItemLabel
              hint="Merged-mining field: Rootstock block header hash"
            >
              Hash for merged mining
            </DetailedInfo.ItemLabel>
            <DetailedInfo.ItemValue
              flexWrap="nowrap"
              alignSelf="flex-start"
            >
              <Box whiteSpace="nowrap" overflow="hidden">
                <HashStringShortenDynamic hash={ data.hash_for_merged_mining }/>
              </Box>
              <CopyToClipboard text={ data.hash_for_merged_mining }/>
            </DetailedInfo.ItemValue>
          </>
        ) }

        { data.difficulty && (
          <>
            <DetailedInfo.ItemLabel
              hint={ `Block difficulty for ${ validatorTitle }, used to calibrate block generation time` }
            >
              Difficulty
            </DetailedInfo.ItemLabel>
            <DetailedInfo.ItemValue>
              <Box overflow="hidden">
                <HashStringShortenDynamic hash={ BigNumber(data.difficulty).toFormat() }/>
              </Box>
            </DetailedInfo.ItemValue>
          </>
        ) }
        { data.total_difficulty && (
          <>
            <DetailedInfo.ItemLabel
              hint="Total difficulty of the chain until this block"
            >
              Total difficulty
            </DetailedInfo.ItemLabel>
            <DetailedInfo.ItemValue>
              <Box overflow="hidden">
                <HashStringShortenDynamic hash={ BigNumber(data.total_difficulty).toFormat() }/>
              </Box>
            </DetailedInfo.ItemValue>
          </>
        ) }

        <DetailedInfo.ItemDivider/>

        <DetailedInfo.ItemLabel
          hint="The SHA256 hash of the block"
        >
          Hash
        </DetailedInfo.ItemLabel>
        <DetailedInfo.ItemValue flexWrap="nowrap">
          <Box overflow="hidden" >
            <HashStringShortenDynamic hash={ data.hash }/>
          </Box>
          <CopyToClipboard text={ data.hash }/>
        </DetailedInfo.ItemValue>

        { data.height > 0 && (
          <>
            <DetailedInfo.ItemLabel
              hint="The hash of the block from which this block was generated"
            >
              Parent hash
            </DetailedInfo.ItemLabel>
            <DetailedInfo.ItemValue flexWrap="nowrap">
              <Link
                href={ route({ pathname: '/block/[height_or_hash]', query: { height_or_hash: String(data.height - 1) } }, multichainContext) }
                overflow="hidden"
                whiteSpace="nowrap"
              >
                <HashStringShortenDynamic
                  hash={ data.parent_hash }
                />
              </Link>
              <CopyToClipboard text={ data.parent_hash }/>
            </DetailedInfo.ItemValue>
          </>
        ) }

        { rollupFeature.isEnabled && rollupFeature.type === 'arbitrum' && data.arbitrum && data.arbitrum.send_count && (
          <>
            <DetailedInfo.ItemLabel
              hint={ `The cumulative number of ${ layerLabels.current } to ${ layerLabels.parent } transactions as of this block` }
              isLoading={ isPlaceholderData }
            >
              Send count
            </DetailedInfo.ItemLabel>
            <DetailedInfo.ItemValue>
              { data.arbitrum.send_count.toLocaleString() }
            </DetailedInfo.ItemValue>

            <DetailedInfo.ItemLabel
              hint={ `The root of the Merkle accumulator representing all ${ layerLabels.current } to ${ layerLabels.parent } transactions as of this block` }
              isLoading={ isPlaceholderData }
            >
              Send root
            </DetailedInfo.ItemLabel>
            <DetailedInfo.ItemValue>
              { data.arbitrum.send_root }
            </DetailedInfo.ItemValue>

            <DetailedInfo.ItemLabel
              hint={ `The number of delayed ${ layerLabels.parent } to ${ layerLabels.current } messages read as of this block` }
              isLoading={ isPlaceholderData }
            >
              Delayed messages
            </DetailedInfo.ItemLabel>
            <DetailedInfo.ItemValue>
              { data.arbitrum.delayed_messages.toLocaleString() }
            </DetailedInfo.ItemValue>
          </>
        ) }

        { !config.UI.views.block.hiddenFields?.nonce && (
          <>
            <DetailedInfo.ItemLabel
              hint="Block nonce is a value used during mining to demonstrate proof of work for a block"
            >
              Nonce
            </DetailedInfo.ItemLabel>
            <DetailedInfo.ItemValue>
              { data.nonce }
            </DetailedInfo.ItemValue>
          </>
        ) }

        { data.zilliqa && (
          <>
            <DetailedInfo.ItemDivider/>
            <BlockDetailsZilliqaQuorumCertificate data={ data.zilliqa?.quorum_certificate }/>
            { data.zilliqa?.aggregate_quorum_certificate && (
              <>
                <GridItem colSpan={{ base: undefined, lg: 2 }} mt={{ base: 1, lg: 2 }}/>
                <BlockDetailsZilliqaQuorumCertificate data={ data.zilliqa?.aggregate_quorum_certificate }/>
              </>
            ) }
          </>
        ) }
      </CollapsibleDetails>

    </DetailedInfo.Container>
  );
};

export default BlockDetails;
