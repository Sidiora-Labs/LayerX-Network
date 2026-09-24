use blockscout_client::{Api, ApiClient, Configuration};
use lazy_static::lazy_static;
use pretty_assertions::assert_eq;
use rstest::*;
use std::{
    sync::{Arc, Mutex, Once},
    time::Duration,
};
use stubr::Stubr;

const DEFAULT_TOKEN_HASH: &str = "0xD4416b13d2b3a9aBae7AcD5D6C2BbDBE25686401";
const DEFAULT_CONTRACT_HASH: &str = "0x8FD4596d4E7788a71F82dAf4119D069a84E7d3f3";
const DEFAULT_TOKEN_INSTANCE_NUMBER: &str =
    "25625468407840116393736812939389551247551040926951238633020744494000165263268";
const DEFAULT_TX_HASH: &str = "0x4dd7e3f4522fcf2483ae422fd007492380051d87de6fdb17be71c7134e26857e";
const DEFAULT_ADDRESS_HASH: &str = "0xc0De20A37E2dAC848F81A93BD85FE4ACDdE7C0DE";
const BLOCK_VALIDATOR_ADDRESS_HASH: &str = "0x4838B106FCe9647Bdf1E7877BF73cE8B0BAD5f97";
const DEFAULT_SEARCH_QUERY: &str = "USDT";

lazy_static! {
    static ref STUBR_SERVER: Arc<Mutex<Option<Stubr>>> = Arc::new(Mutex::new(None));
    static ref INIT: Once = Once::new();
}

// NOTE: little hack, I do not know how to make it work without this.
// Stubr::start_blocking does not work, because it uses async_std::task::block_on,
// and it make tests flaky, because it drops the server before the test is finished :(
// TODO: find a better way to do this.
#[fixture]
async fn api_client() -> ApiClient {
    INIT.call_once(|| {
        tokio::spawn(async {
            let server = Stubr::try_start("tests/recorded/eth_blockscout_com")
                .await
                .unwrap();
            *STUBR_SERVER.lock().unwrap() = Some(server);
        });
    });
    tokio::time::sleep(Duration::from_secs(1)).await;

    let server = STUBR_SERVER.lock().unwrap();
    let server = server.as_ref().expect("Server not initialized");
    api_client_from_uri(server.uri().as_str())
}

fn api_client_from_uri(uri: &str) -> ApiClient {
    ApiClient::new(Configuration::builder().base_path(uri).build())
}

#[rstest]
#[tokio::test]
async fn health(#[future] api_client: ApiClient) {
    let api_client = api_client.await;
    let health_api = api_client.health_api();
    let health = health_api.health().await.expect("Failed to get health");
    let success = health.try_as_success().cloned().unwrap_or_else(|| {
        panic!("failed to get health: {:?}", &health.entity);
    });
    assert_eq!(success.healthy, Some(true));
    assert!(success
        .metadata
        .expect("metadata is None")
        .latest_block
        .expect("latest_block is None")
        .db
        .expect("db is None")
        .number
        .expect("number is None")
        .chars()
        .all(|c| c.is_ascii_digit()),);
    let health_v1 = health_api.health_v1().await.expect("Failed to get health");
    let success = health_v1.try_as_success().cloned().unwrap_or_else(|| {
        panic!("failed to get health: {:?}", &health_v1.entity);
    });
    assert_eq!(success.healthy, Some(true));
    assert_eq!(
        success
            .data
            .expect("data is None")
            .latest_block_number
            .expect("latest_block_number is None"),
        "21879216"
    );
}

#[rstest]
#[tokio::test]
async fn blocks(#[future] api_client: ApiClient) {
    let api_client = api_client.await;
    let blocks_api = api_client.blocks_api();
    let blocks = blocks_api
        .get_blocks(blockscout_client::apis::blocks_api::GetBlocksParams::builder().build())
        .await
        .expect("Failed to get blocks");
    let success_model = blocks.try_parse_as_success_model().unwrap();
    assert!(!success_model.items.is_empty());
}

#[rstest]
#[tokio::test]
async fn transactions(#[future] api_client: ApiClient) {
    let api_client = api_client.await;
    let transactions_api = api_client.transactions_api();
    let transactions = transactions_api
        .get_txs(blockscout_client::apis::transactions_api::GetTxsParams::builder().build())
        .await
        .expect("Failed to get transactions");
    let success_model = transactions.try_parse_as_success_model().unwrap();
    assert!(!success_model.items.is_empty());
}

#[rstest]
#[tokio::test]
async fn internal_transactions(#[future] api_client: ApiClient) {
    let api_client = api_client.await;
    let transactions_api = api_client.transactions_api();
    let internal_transactions = transactions_api
        .get_transaction_internal_txs(
            blockscout_client::apis::transactions_api::GetTransactionInternalTxsParams::builder()
                .transaction_hash(DEFAULT_TX_HASH.to_owned())
                .build(),
        )
        .await
        .expect("Failed to get transactions");
    let success_model = internal_transactions.try_parse_as_success_model().unwrap();
    assert!(!success_model.items.is_empty());
}

#[rstest]
#[tokio::test]
async fn smart_contracts(#[future] api_client: ApiClient) {
    let api_client = api_client.await;
    let smart_contracts_api = api_client.smart_contracts_api();
    let smart_contracts = smart_contracts_api
        .get_smart_contracts(
            blockscout_client::apis::smart_contracts_api::GetSmartContractsParams::builder()
                .build(),
        )
        .await
        .expect("Failed to get transactions");
    let success_model = smart_contracts.try_parse_as_success_model().unwrap();
    assert!(!success_model.items.is_empty());
    let _smart_contract = smart_contracts_api
        .get_smart_contract(
            blockscout_client::apis::smart_contracts_api::GetSmartContractParams::builder()
                .address_hash(DEFAULT_CONTRACT_HASH.to_owned())
                .build(),
        )
        .await
        .expect("Failed to get transactions");
}

#[rstest]
#[tokio::test]
async fn tokens(#[future] api_client: ApiClient) {
    let api_client = api_client.await;
    let tokens_api = api_client.tokens_api();
    let tokens = tokens_api
        .get_tokens_list(
            blockscout_client::apis::tokens_api::GetTokensListParams::builder().build(),
        )
        .await
        .expect("Failed to get transactions");
    let success_model = tokens.try_parse_as_success_model().unwrap();
    assert!(!success_model.items.is_empty());

    let _token = tokens_api
        .get_token(
            blockscout_client::apis::tokens_api::GetTokenParams::builder()
                .address_hash(DEFAULT_TOKEN_HASH.to_owned())
                .build(),
        )
        .await
        .expect("Failed to get transactions")
        .try_parse_as_success_model()
        .unwrap();
    let token_instances = tokens_api
        .get_nft_instances(
            blockscout_client::apis::tokens_api::GetNftInstancesParams::builder()
                .address_hash(DEFAULT_TOKEN_HASH.to_owned())
                .build(),
        )
        .await
        .expect("Failed to get transactions");
    let success_model = token_instances.try_parse_as_success_model().unwrap();
    assert!(!success_model.items.is_empty());

    let params = blockscout_client::apis::tokens_api::GetNftInstanceParams::builder()
        .address_hash(DEFAULT_TOKEN_HASH.to_owned())
        .id(DEFAULT_TOKEN_INSTANCE_NUMBER.to_owned())
        .build();
    let _token_instance = tokens_api
        .get_nft_instance(params)
        .await
        .expect("Failed to get transactions")
        .try_parse_as_success_model()
        .unwrap();
}

#[rstest]
#[tokio::test]
async fn get_transaction(#[future] api_client: ApiClient) {
    let api_client = api_client.await;
    let transactions_api = api_client.transactions_api();
    let response = transactions_api
        .get_tx(
            blockscout_client::apis::transactions_api::GetTxParams::builder()
                .transaction_hash(DEFAULT_TX_HASH.to_owned())
                .build(),
        )
        .await
        .expect("Failed to get transactions");
    let success_model = response.try_parse_as_success_model().unwrap();
    assert_eq!(success_model.hash, DEFAULT_TX_HASH);
}

#[rstest]
#[tokio::test]
async fn token_counters(#[future] api_client: ApiClient) {
    let api_client = api_client.await;
    let tokens_api = api_client.tokens_api();
    let response = tokens_api
        .get_token_counters(
            blockscout_client::apis::tokens_api::GetTokenCountersParams::builder()
                .address_hash(DEFAULT_TOKEN_HASH.to_owned())
                .build(),
        )
        .await
        .expect("Failed to get token counters");
    let success_model = response.try_parse_as_success_model().unwrap();
    assert!(success_model.transfers_count.parse::<i64>().unwrap() > 0);
}

#[rstest]
#[tokio::test]
async fn token_holders(#[future] api_client: ApiClient) {
    let api_client = api_client.await;
    let tokens_api = api_client.tokens_api();
    let response = tokens_api
        .get_token_holders(
            blockscout_client::apis::tokens_api::GetTokenHoldersParams::builder()
                .address_hash(DEFAULT_TOKEN_HASH.to_owned())
                .build(),
        )
        .await
        .expect("Failed to get token holders");
    let success_model = response.try_parse_as_success_model().unwrap();
    assert!(!success_model.items.is_empty());
}

#[rstest]
#[tokio::test]
async fn token_transfers(#[future] api_client: ApiClient) {
    let api_client = api_client.await;
    let tokens_api = api_client.tokens_api();
    let response = tokens_api
        .get_token_token_transfers(
            blockscout_client::apis::tokens_api::GetTokenTokenTransfersParams::builder()
                .address_hash(DEFAULT_TOKEN_HASH.to_owned())
                .build(),
        )
        .await
        .expect("Failed to get token transfers");
    let success_model = response.try_parse_as_success_model().unwrap();
    assert!(!success_model.items.is_empty());
}

#[rstest]
#[tokio::test]
async fn token_instance_holders(#[future] api_client: ApiClient) {
    let api_client = api_client.await;
    let tokens_api = api_client.tokens_api();
    let response = tokens_api
        .get_token_instance_holders(
            blockscout_client::apis::tokens_api::GetTokenInstanceHoldersParams::builder()
                .address_hash(DEFAULT_TOKEN_HASH.to_owned())
                .id(DEFAULT_TOKEN_INSTANCE_NUMBER.to_owned())
                .build(),
        )
        .await
        .expect("Failed to get token instance holders");
    let success_model = response.try_parse_as_success_model().unwrap();
    assert!(!success_model.items.is_empty());
}

#[rstest]
#[tokio::test]
async fn address_details(#[future] api_client: ApiClient) {
    let api_client = api_client.await;
    let addresses_api = api_client.addresses_api();
    let response = addresses_api
        .get_address(
            blockscout_client::apis::addresses_api::GetAddressParams::builder()
                .address_hash(DEFAULT_ADDRESS_HASH.to_owned())
                .build(),
        )
        .await
        .expect("Failed to get address details");
    let success_model = response.try_parse_as_success_model().unwrap();
    assert_eq!(success_model.hash, DEFAULT_ADDRESS_HASH);
}

#[rstest]
#[tokio::test]
async fn address_blocks_validated(#[future] api_client: ApiClient) {
    let api_client = api_client.await;
    let addresses_api = api_client.addresses_api();
    let response = addresses_api
        .get_address_blocks_validated(
            blockscout_client::apis::addresses_api::GetAddressBlocksValidatedParams::builder()
                .address_hash(BLOCK_VALIDATOR_ADDRESS_HASH.to_owned())
                .build(),
        )
        .await
        .expect("Failed to get address blocks validated");
    let success_model = response.try_parse_as_success_model().unwrap();
    assert!(!success_model.items.is_empty());
}

#[rstest]
#[tokio::test]
async fn address_counters(#[future] api_client: ApiClient) {
    let api_client = api_client.await;
    let addresses_api = api_client.addresses_api();
    let response = addresses_api
        .get_address_counters(
            blockscout_client::apis::addresses_api::GetAddressCountersParams::builder()
                .address_hash(DEFAULT_ADDRESS_HASH.to_owned())
                .build(),
        )
        .await
        .expect("Failed to get address counters");
    let success_model = response.try_parse_as_success_model().unwrap();
    assert!(success_model.transactions_count.parse::<i64>().unwrap() > 0);
}

#[rstest]
#[tokio::test]
async fn address_logs(#[future] api_client: ApiClient) {
    let api_client = api_client.await;
    let addresses_api = api_client.addresses_api();
    let response = addresses_api
        .get_address_logs(
            blockscout_client::apis::addresses_api::GetAddressLogsParams::builder()
                .address_hash(DEFAULT_CONTRACT_HASH.to_owned())
                .build(),
        )
        .await
        .expect("Failed to get address logs");
    let success_model = response.try_parse_as_success_model().unwrap();
    assert!(!success_model.items.is_empty());
}

#[rstest]
#[tokio::test]
async fn address_nft_collections(#[future] api_client: ApiClient) {
    let api_client = api_client.await;
    let addresses_api = api_client.addresses_api();
    let response = addresses_api
        .get_address_nft_collections(
            blockscout_client::apis::addresses_api::GetAddressNftCollectionsParams::builder()
                .address_hash(DEFAULT_ADDRESS_HASH.to_owned())
                .build(),
        )
        .await
        .expect("Failed to get address NFT collections");
    let success_model = response.try_parse_as_success_model().unwrap();
    assert!(!success_model.items.is_empty());
}

#[rstest]
#[tokio::test]
async fn address_token_balances(#[future] api_client: ApiClient) {
    let api_client = api_client.await;
    let addresses_api = api_client.addresses_api();
    let response = addresses_api
        .get_address_token_balances(
            blockscout_client::apis::addresses_api::GetAddressTokenBalancesParams::builder()
                .address_hash(DEFAULT_ADDRESS_HASH.to_owned())
                .build(),
        )
        .await
        .expect("Failed to get address token balances");
    let success_model = response.try_parse_as_success_model().unwrap();
    assert!(!success_model.is_empty());
}

#[rstest]
#[tokio::test]
async fn address_token_transfers(#[future] api_client: ApiClient) {
    let api_client = api_client.await;
    let addresses_api = api_client.addresses_api();
    let response = addresses_api
        .get_address_token_transfers(
            blockscout_client::apis::addresses_api::GetAddressTokenTransfersParams::builder()
                .address_hash(DEFAULT_ADDRESS_HASH.to_owned())
                .build(),
        )
        .await
        .expect("Failed to get address token transfers");
    let success_model = response.try_parse_as_success_model().unwrap();
    assert!(!success_model.items.is_empty());
}

#[rstest]
#[tokio::test]
async fn address_tokens(#[future] api_client: ApiClient) {
    let api_client = api_client.await;
    let addresses_api = api_client.addresses_api();
    let response = addresses_api
        .get_address_tokens(
            blockscout_client::apis::addresses_api::GetAddressTokensParams::builder()
                .address_hash(DEFAULT_ADDRESS_HASH.to_owned())
                .build(),
        )
        .await
        .expect("Failed to get address tokens");
    let success_model = response.try_parse_as_success_model().unwrap();
    assert!(!success_model.items.is_empty());
}

#[rstest]
#[tokio::test]
async fn stats(#[future] api_client: ApiClient) {
    let api_client = api_client.await;
    let stats_api = api_client.stats_api();
    let response = stats_api.get_stats().await.expect("Failed to get stats");
    let success_model = response.try_parse_as_success_model().unwrap();
    assert!(success_model.total_blocks.unwrap().parse::<i64>().unwrap() > 0);
}

// #[rstest]
// #[tokio::test]
// async fn config_json_rpc_url(api_client: &ApiClient) {
//     let config_api = api_client.config_api();
//     let response = config_api
//         .get_json_rpc_url()
//         .await
//         .expect("Failed to get JSON RPC URL");
//     let success_model = response.try_parse_as_success_model().unwrap();
//     assert!(!success_model.json_rpc_url.is_empty());
// }

#[rstest]
#[tokio::test]
async fn search(#[future] api_client: ApiClient) {
    let api_client = api_client.await;
    let search_api = api_client.search_api();
    let params = blockscout_client::apis::search_api::SearchParams::builder()
        .q(DEFAULT_SEARCH_QUERY.to_owned())
        .build();
    let response = search_api
        .search(params)
        .await
        .expect("Failed to get search");
    let success_model = response.try_parse_as_success_model().unwrap();
    assert!(!success_model.items.is_empty());
}

use serde::{Deserialize, Serialize};

#[rstest]
#[tokio::test]
async fn deserialize_decimal() {
    #[derive(Debug, Serialize, Deserialize)]
    struct Message {
        maybe_number: Option<rust_decimal::Decimal>,
        number: rust_decimal::Decimal,
        numbers: Vec<rust_decimal::Decimal>,
    }

    let _: Message = serde_json::from_str(
        r#"{"maybe_number": 0, "number": "1.23", "numbers": ["1.23", "4.56", "0"]}"#,
    )
    .unwrap();
}
