import config from 'configs/app';
import { writeFileSync } from 'node:fs';
import { dirname, resolve as resolvePath } from 'node:path';
import { fileURLToPath } from 'node:url';
import dedent from 'dedent';
import { layerLabels } from 'lib/rollups/utils';

const currentFilePath = fileURLToPath(import.meta.url);
const currentDir = dirname(currentFilePath);
const outputDir = resolvePath(currentDir, '../../../../public');
const outputFile = resolvePath(outputDir, 'llms.txt');

function run() {
    try {
        if(config.features.multichain.isEnabled){
            console.log('⏭️ Skipping llms.txt generation for multichain explorer');
            return;
        }

        console.log('🌀 Generating llms.txt...');
            
        const chainName = config.chain.name ?? '';
        const chainId = config.chain.id ?? '';
        const generalApiUrl = config.apis.general? config.apis.general.endpoint + config.apis.general.basePath : '';
        const statsApiUrl = config.apis.stats ? config.apis.stats.endpoint + config.apis.stats.basePath : undefined;

        const rollupFeature = config.features.rollup;
        const parentChainUrl = rollupFeature.isEnabled ? rollupFeature.parentChain.baseUrl : undefined;
        const currentToParentLayerLabel = layerLabels.current + '→' + layerLabels.parent;
        const parentToCurrentLayerLabel = layerLabels.parent + '→' + layerLabels.current;
        
        const validatorsFeature = config.features.validators;

        const MCP_SERVER_URL = 'https://mcp.blockscout.com';

        const GENERAL_COUNTERS_TEMPLATE = statsApiUrl ? `
            ### General Counters

            \`\`\`bash
            curl --request GET --url '${statsApiUrl}/api/v1/counters'
            \`\`\`
        ` : '{blank}';

        const USER_OPS_TEMPLATE = config.features.userOps.isEnabled ? `
            <!-- START:Account-Abstraction -->
            ### Account Abstraction info

            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/proxy/account-abstraction/accounts/{account_address}'
            \`\`\`

            ### User Operations by Address

            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/proxy/account-abstraction/operations?sender={account_address}'
            \`\`\`

            ### User Operations by Transaction

            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/proxy/account-abstraction/operations?transaction_hash={transaction_hash}'
            \`\`\`

            ### User Operation Details

            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/proxy/account-abstraction/operations/{user_operation_hash}'
            \`\`\`
            <!-- END:Account-Abstraction -->
        ` : '{blank}';

        const BEACON_CHAIN_TEMPLATE = config.features.beaconChain.isEnabled ? `
            ### Deposits by Address
                
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/addresses/{account_address}/beacon/deposits'
            \`\`\`
                
            ### Deposits by Block
                
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/block/{block_number}/beacon/deposits'
            \`\`\`

            ### Withdrawals by Address

            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/addresses/{account_address}/withdrawals'
            \`\`\`

            ### Withdrawals by Block

            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/blocks/{block_number}/withdrawals'
            \`\`\`
        ` : undefined;

        const ARBITRUM_CHAIN_TEMPLATE = rollupFeature.isEnabled && rollupFeature.type === 'arbitrum' ? `
            ### Latest Committed Batch Number

            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/main-page/arbitrum/batches/latest-number'
            \`\`\`

            ### Batch Info
                            
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/arbitrum/batches/{batch_number}'
            \`\`\`
                            
            ### Blocks By Batch
                            
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/blocks/arbitrum-batch/{batch_number}'
            \`\`\`
                            
            ### Get ${ parentToCurrentLayerLabel } messages
                            
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/arbitrum/messages/to-rollup'
            \`\`\`
                            
            ### Get ${ currentToParentLayerLabel } messages
                            
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/arbitrum/messages/from-rollup'
            \`\`\`
                            
            ### ${ currentToParentLayerLabel } messages by transaction:
                            
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/arbitrum/messages/withdrawals/{transactions_hash}'
            \`\`\`
        ` : undefined;

        const OPTIMISM_CHAIN_TEMPLATE = rollupFeature.isEnabled && rollupFeature.type === 'optimistic' ? `
            ### Latest Committed Batch Number (top of)
                
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/optimism/batches'
            \`\`\`
                
            ### Batch Info
                
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/optimism/batches/{batch_number}'
            \`\`\`
                
            ### Blocks By Batch (TODO: not needed if the batch returns blocks range)
                
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/blocks/optimism-batch/{batch_number}'
            \`\`\`
                
            ### Dispute Games
                
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/optimism/games'
            \`\`\`
                
            ### Get ${ parentToCurrentLayerLabel } messages
                
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/optimism/deposits'
            \`\`\`
                
            ### Get ${ currentToParentLayerLabel } messages
                
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/optimism/withdrawals'
            \`\`\`
        ` : undefined;

        const CELO_CHAIN_TEMPLATE = config.features.celo.isEnabled ? `
            ### Latest Finalized Epoch (top of)
                
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/celo/epochs'
            \`\`\`
                
            ### Get Epoch Information
                
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/celo/epochs/{epoch_number}'
            \`\`\`
                
            ### Validator Group Reward by Epoch
                
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/celo/epochs/{epoch_number}/election-rewards/group'
            \`\`\`
                
            ### Validator Rewards by Epoch
                
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/celo/epochs/{epoch_number}/election-rewards/validator'
            \`\`\`
                
            ### Voting Rewards by Epoch
                
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/celo/epochs/{epoch_number}/election-rewards/voter'
            \`\`\`
        ` : undefined;

        const ZKSYNC_CHAIN_TEMPLATE = rollupFeature.isEnabled && rollupFeature.type === 'zkSync' ? `
            ### Latest Committed Batch Number
                
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/main-page/zksync/batches/latest-number'
            \`\`\`
                
            ### Batch info
                
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/zksync/batches/{batch_number}'
            \`\`\`
        ` : undefined;

        const ZKEVM_CHAIN_TEMPLATE = rollupFeature.isEnabled && rollupFeature.type === 'zkEvm' ? `
            ### Latest Committed Batch Number (top of)
                
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/zkevm/batches/confirmed'
            \`\`\`
                
            ### Batch Info

            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/zkevm/batches/{batch_number}'
            \`\`\`
                
            ### Deposits
                
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/zkevm/deposits'
            \`\`\`
                
            ### Withdrawals
                
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/zkevm/withdrawals'
            \`\`\`
        ` : undefined;

        const TAC_CHAIN_TEMPLATE = config.features.tac.isEnabled && config.apis.tac ? `
            ### TAC Operations:
                
            \`\`\`bash
            curl --request GET --url '${config.apis.tac.endpoint }/api/v1/tac/operations'
            \`\`\`
                
            ### TAC Operation Info:
                
            \`\`\`bash
            curl --request GET --url '${config.apis.tac.endpoint }/api/v1/tac/operations/{operation_id}'
            \`\`\`
        ` : undefined;

        const REDSTONE_CHAIN_TEMPLATE = config.features.mudFramework.isEnabled ? `
            ### MUD Worlds
                
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/mud/worlds'
            \`\`\`
                
            ### MUD World Tables
                
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/mud/worlds/{contract_address}/tables'
            \`\`\`
                
            ### MUD World Table Records
                
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/mud/worlds/{contract_address}/tables/{table_id}/records'
            \`\`\`
                
            ### MUD World Table Record
                
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/mud/worlds/{contract_address}/tables/{table_id}/records/{record_id}'
            \`\`\`
        ` : undefined;

        const SCROLL_CHAIN_TEMPLATE = rollupFeature.isEnabled && rollupFeature.type === 'scroll' ? `
            ### Latest Committed Batch Number (top of)
                
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/scroll/batches'
            \`\`\`
                
            ### Batch Info
                
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/scroll/batches/{batch_number}'
            \`\`\`
                
            ### Blocks By Batch
                
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/blocks/scroll-batch/{batch_number}'
            \`\`\`
                
            ### Deposits (${ parentToCurrentLayerLabel })
                
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/scroll/deposits'
            \`\`\`
                
            ### Withdrawals (${ currentToParentLayerLabel })
                
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/scroll/withdrawals'
            \`\`\`
        ` : undefined;

        const SHIBARIUM_CHAIN_TEMPLATE = rollupFeature.isEnabled && rollupFeature.type === 'shibarium' ? `
            ### Deposits (${ parentToCurrentLayerLabel })
                
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/shibarium/deposits'
            \`\`\`
                
            ### Withdrawals (${ currentToParentLayerLabel })
                
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/shibarium/withdrawals'
            \`\`\`
        ` : undefined;

        const ZILLIQA_CHAIN_TEMPLATE = validatorsFeature.isEnabled && validatorsFeature.chainType === 'zilliqa' ? `
            ### Validators list:
                
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/validators/zilliqa'
            \`\`\`
                
            ### Validator Info:
                
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/validators/zilliqa/{validator_public_key}'
            \`\`\`
        ` : undefined;

        const STABILITY_CHAIN_TEMPLATE = validatorsFeature.isEnabled && validatorsFeature.chainType === 'stability' ? `
            ### Validators list:
                
            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/validators/stability'
            \`\`\`
        ` : undefined;

        const CHAIN_SPECIFIC_DATA = [
            BEACON_CHAIN_TEMPLATE,
            ARBITRUM_CHAIN_TEMPLATE,
            OPTIMISM_CHAIN_TEMPLATE,
            CELO_CHAIN_TEMPLATE,
            ZKSYNC_CHAIN_TEMPLATE,
            ZKEVM_CHAIN_TEMPLATE,
            TAC_CHAIN_TEMPLATE,
            REDSTONE_CHAIN_TEMPLATE,
            SCROLL_CHAIN_TEMPLATE,
            ZILLIQA_CHAIN_TEMPLATE,
            STABILITY_CHAIN_TEMPLATE,
            SHIBARIUM_CHAIN_TEMPLATE
        ].filter(Boolean);

        const CHAIN_SPECIFIC_TEMPLATE = CHAIN_SPECIFIC_DATA.length > 0 ? `
            <!-- START:Chain-Specific-Data -->
            ## Chain-Specific Data
            ${ CHAIN_SPECIFIC_DATA.join('\n') }
            <!-- END:Chain-Specific--Data -->
        ` : '{blank}';

        const MAIN_TEMPLATE = dedent`
            # Blockscout - ${ chainName }

            <!-- START:Blockscout-Common-Intro -->
            Blockscout is a human-friendly blockchain explorer for EVM-compatible networks. It lets users browse blocks, transactions, addresses, tokens (ERC-20/721/1155), logs, contract ABIs, and decoded contract interactions. While the site is primarily designed for people, all rendered information is backed by API endpoints that machines can consume. Large Language Models should prefer the LLM-ready endpoints listed below to retrieve precise, structured data.
            <!-- END:Blockscout-Common-Intro -->

            <!-- START:Blockscout-Instance-Info -->
            Chain name: ${ chainName }
            Chain ID: ${ chainId }
            ${ parentChainUrl ? `Settlement layer Blockscout URL: ${ parentChainUrl }` : '{blank}' }
            <!-- END:Blockscout-Instance-Info -->

            <!-- START:General-LLM-Ready-Blockchain-Data -->
            ## General LLM-ready Data

            The links below lead to the REST API of the Blockscout MCP server, they are considered as LLM-ready. More detail: https://github.com/blockscout/mcp-server/blob/main/API.md

            <!-- START:General-Data-Blocks -->
            ### Specific Block Info

            \`\`\`bash
            curl --request GET --url '${ MCP_SERVER_URL }/v1/get_block_info?chain_id=${ chainId }&number_or_hash={block_number_or_hash}&include_transactions=false'
            \`\`\`
            <!-- END:General-Data-Blocks -->

            <!-- START:General-Data-Transactions -->
            ### Specific Transaction Info

            \`\`\`bash
            curl --request GET --url '${ MCP_SERVER_URL }/v1/get_transaction_info?chain_id=${ chainId }&transaction_hash={transaction_hash}'
            \`\`\`

            ### Get Transaction Logs

            \`\`\`bash
            curl --request GET --url '${ MCP_SERVER_URL }/v1/get_transaction_logs?chain_id=${ chainId }&transaction_hash={transaction_hash}'
            \`\`\`

            ### Transaction Summary

            \`\`\`bash
            curl --request GET --url '${ MCP_SERVER_URL }/v1/transaction_summary?chain_id=${ chainId }&transaction_hash={transaction_hash}'
            \`\`\`
            <!-- END:General-Data-Transactions -->

            <!-- START:General-Data-Addresses -->
            ### Specific Account info

            \`\`\`bash
            curl --request GET --url '${ MCP_SERVER_URL }/v1/get_address_info?chain_id=${ chainId }&address={account_address}'
            \`\`\`

            ### Get Address by ENS name

            \`\`\`bash
            curl --request GET --url '${ MCP_SERVER_URL }/v1/get_address_by_ens_name?name={ens_name}'
            \`\`\`

            ### Get Transactions By Address

            \`\`\`bash
            curl --request GET --url '${ MCP_SERVER_URL }/v1/get_transactions_by_address?chain_id=${ chainId }&address={account_address}&age_from={YYYY-MM-DDTHH:MM:SS.00Z}&age_to={YYYY-MM-DDTHH:MM:SS.00Z}&methods={4_bytes_method_signature_optional}'
            \`\`\`

            ### Get Token Transfers by Address

            \`\`\`bash
            curl --request GET --url '${ MCP_SERVER_URL }/v1/get_token_transfers_by_address?chain_id=${ chainId }&address={account_address}&age_from={YYYY-MM-DDTHH:MM:SS.00Z}&age_to={YYYY-MM-DDTHH:MM:SS.00Z}&token={token_contract_address_optional}'
            \`\`\`
            <!-- END:General-Data-Addresses -->

            <!-- START:General-Data-Tokens -->
            ### Get ERC20 Tokens By Address

            \`\`\`bash
            curl --request GET --url '${ MCP_SERVER_URL }/v1/get_tokens_by_address?chain_id=${ chainId }&address={account_address}'
            \`\`\`

            ### Get NFT Tokens By Address

            \`\`\`bash
            curl --request GET --url '${ MCP_SERVER_URL }/v1/nft_tokens_by_address?chain_id=${ chainId }&address={account_address}'
            \`\`\`

            ### Lookup Token By Symbol

            \`\`\`bash
            curl --request GET --url '${ MCP_SERVER_URL }/v1/lookup_token_by_symbol?chain_id=${ chainId }&symbol={token_symbol}'
            \`\`\`
            <!-- END:General-Data-Tokens -->

            <!-- START:General-Data-Contracts -->
            ### Get Contract ABI

            \`\`\`bash
            curl --request GET --url '${ MCP_SERVER_URL }/v1/get_contract_abi?chain_id=${ chainId }&address={contract_address}'
            \`\`\`

            ### Read Contract Data

            \`\`\`bash
            curl --request GET --url '${ MCP_SERVER_URL }/v1/read_contract?chain_id=${ chainId }&address={contract_address}&abi={string_encoded_function_abi}&function_name={function_name}&args={string_encoded_arguments_list}'
            \`\`\`
            
            ### Get Source Files Tree of Verified Contract
            
            \`\`\`bash
            curl --request GET --url '${ MCP_SERVER_URL }/v1/inspect_contract_code?chain_id=${ chainId }&address={contract_address}'
            \`\`\`
            
            ### Inspect Specific Source File of Verified Contract
            
            \`\`\`bash
            curl --request GET --url '${ MCP_SERVER_URL }/v1/inspect_contract_code?chain_id=${ chainId }&address={contract_address}&file_name={file_name}'
            \`\`\`
            <!-- END:General-Data-Contracts -->

            <!-- END:General-LLM-Ready-Blockchain-Data -->

            <!-- START:Miscellaneous-Blockchain-Data -->
            ## Miscellaneous Blockchain Data

            <!-- START:Stats -->
            ${ GENERAL_COUNTERS_TEMPLATE }
            ### Gas Tracker

            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/stats'
            \`\`\`
            <!-- END:Stats -->

            <!-- START:Addresses -->
            ### Coin Balance History by Address

            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/addresses/{account_address}/coin-balance-history-by-day'
            curl --request GET --url '${ generalApiUrl }/api/v2/addresses/{account_address}/coin-balance-history'
            \`\`\`

            ### Logs Emitted by Address

            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/addresses/{account_address}/logs'
            \`\`\`

            ### Blocks Validated by Address

            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/addresses/{account_address}/blocks-validated'
            \`\`\`
            <!-- END:Addresses -->
            ${USER_OPS_TEMPLATE}
            <!-- START:Tokens -->
            ### Holders By Token Address

            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/tokens/{token_contract_address}/holders'
            \`\`\`

            ### NFT Inventory By Token Address

            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/tokens/{token_contract_address}/instances'
            \`\`\`

            ### NFT Instance Info

            \`\`\`bash
            curl --request GET --url '${ generalApiUrl }/api/v2/tokens/{token_contract_address}/instances/{instance_id}'
            curl --request GET --url '${ generalApiUrl }/api/v2/tokens/{token_contract_address}/instances/{instance_id}/transfers'
            \`\`\`
            <!-- END:Tokens -->

            <!-- END:Miscellaneous-Blockchain-Data -->
            ${ CHAIN_SPECIFIC_TEMPLATE }
            <!-- START:Additional-Info -->
            ## Additional Info

            **Recommendation**: For programmatic access and LLM workflows, prefer the MCP Server endpoints (HTTP, not SSE).

            - MCP landing: [${ MCP_SERVER_URL }](${ MCP_SERVER_URL })
            - MCP server root: [${ MCP_SERVER_URL }/mcp/](${ MCP_SERVER_URL }/mcp/)

            ### Other Blockscout instances (lookup by chain id or name):

            \`\`\`bash
            curl --request GET --url 'https://chains.blockscout.com/api/chains'
            \`\`\`
            <!-- END:Additional-Info -->
        `;

        const content = MAIN_TEMPLATE.replace('{blank}\n', '');
        writeFileSync(outputFile, content);

        console.log('👍 Done!\n');
    } catch (error) {
        console.error('🚨 Error generating llms.txt:', error);
        console.log('\n');
        process.exit(1);
    }
}

run();