use blockscout_service_launcher::test_server;
use httpmock::{Mock, MockServer, Then, When};
use pretty_assertions::assert_eq;
use serde_json::Value;
use sig_provider_server::{EthBytecodeDbSettings, SourcesSettings};
use std::{cell::RefCell, time::Duration};

async fn run_server(sources: &SourceMocks<'_>) -> url::Url {
    let mut settings = sig_provider_server::Settings::default();
    let (server_settings, base) = test_server::get_test_server_settings();
    settings.server = server_settings;
    settings.jaeger.enabled = false;
    settings.tracing.enabled = false;

    settings.sources = SourcesSettings {
        fourbyte: format!("http://127.0.0.1:{}/", sources.fourbyte.port())
            .parse()
            .unwrap(),
        sigeth: format!("http://127.0.0.1:{}/", sources.sigeth.port())
            .parse()
            .unwrap(),
        eth_bytecode_db: EthBytecodeDbSettings {
            enabled: true,
            url: format!("http://127.0.0.1:{}/", sources.eth_bytecode_db.port())
                .parse()
                .unwrap(),
        },
    };
    test_server::init_server(|| sig_provider_server::sig_provider(settings), &base).await;
    base
}

struct SourceMocks<'a> {
    fourbyte: MockServer,
    sigeth: MockServer,
    eth_bytecode_db: MockServer,

    mocks: RefCell<Vec<Mock<'a>>>,
}

impl<'a> SourceMocks<'a> {
    pub fn new() -> Self {
        Self {
            fourbyte: MockServer::start(),
            sigeth: MockServer::start(),
            eth_bytecode_db: MockServer::start(),

            mocks: RefCell::new(Vec::new()),
        }
    }

    pub fn fourbyte_mock<F>(&'a self, config_fn: F)
    where
        F: FnOnce(When, Then),
    {
        self.mocks.borrow_mut().push(self.fourbyte.mock(config_fn));
    }

    pub fn sigeth_mock<F>(&'a self, config_fn: F)
    where
        F: FnOnce(When, Then),
    {
        self.mocks.borrow_mut().push(self.sigeth.mock(config_fn));
    }

    pub fn eth_bytecode_db_mock<F>(&'a self, config_fn: F)
    where
        F: FnOnce(When, Then),
    {
        self.mocks
            .borrow_mut()
            .push(self.eth_bytecode_db.mock(config_fn));
    }

    pub fn assert(&self) {
        for mock in self.mocks.borrow().iter() {
            mock.assert();
        }
    }
}

#[tokio::test]
async fn create() {
    let _ = tracing_subscriber::fmt::try_init();

    let mocks = SourceMocks::new();

    let fourbyte_request = serde_json::json!({"contract_abi":"[{\"constant\":false,\"inputs\":[],\"name\":\"f\",\"outputs\":[],\"type\":\"function\"},{\"inputs\":[],\"type\":\"constructor\"},{\"anonymous\":false,\"inputs\":[{\"name\":\"\",\"type\":\"string\",\"indexed\":true}],\"name\":\"E\",\"type\":\"event\"}]"});
    let fourbyte_response = serde_json::json!({"num_processed":25,"num_imported":3,"num_duplicates":18,"num_ignored":4});
    mocks.fourbyte_mock(|when, then| {
        when.method(httpmock::Method::POST)
            .path("/api/v1/import-solidity/")
            .header("Content-type", "application/json")
            .json_body(fourbyte_request);
        then.status(201)
            .header("Content-type", "application/json")
            .json_body(fourbyte_response);
    });

    let sigeth_request = serde_json::json!({"type":"abi","data":[[{"constant":false,"inputs":[],"name":"f","outputs":[],"type":"function"},{"inputs":[],"type":"constructor"},{"anonymous":false,"inputs":[{"name":"","type":"string","indexed":true}],"name":"E","type":"event"}]]});
    let sigeth_response = serde_json::json!({"ok":true,"result":{"event":{"imported":{},"duplicated":{"E(string)":"0x3e9992c940c54ea252d3a34557cc3d3014281525c43d694f89d5f3dfd820b07d"},"invalid":null},"function":{"imported":{},"duplicated":{"f()":"0x26121ff0"},"invalid":null}}});
    mocks.sigeth_mock(|when, then| {
        when.method(httpmock::Method::POST)
            .path("/api/v1/import")
            .header("Content-type", "application/json")
            .json_body(sigeth_request);
        then.status(200)
            .header("Content-type", "application/json")
            .json_body(sigeth_response);
    });

    let base = run_server(&mocks).await;

    let route = "/api/v1/signatures";
    let request = serde_json::json!({"abi":"[{\"constant\":false,\"inputs\":[],\"name\":\"f\",\"outputs\":[],\"type\":\"function\"},{\"inputs\":[],\"type\":\"constructor\"},{\"anonymous\":false,\"inputs\":[{\"name\":\"\",\"type\":\"string\",\"indexed\":true}],\"name\":\"E\",\"type\":\"event\"}]"});
    let response: serde_json::Value = test_server::send_post_request(&base, route, &request).await;
    // allow async handle to work
    tokio::time::sleep(Duration::from_millis(100)).await;

    mocks.assert();

    assert_eq!(serde_json::json!({}), response);
}

fn sort_json_mut(v: &mut Value) {
    match v {
        Value::Array(arr) => {
            arr.sort_by_key(|v| v.to_string());
        }
        Value::Object(obj) => {
            for (_, val) in obj.iter_mut() {
                sort_json_mut(val);
            }
        }
        _ => (),
    }
}

fn sort_json(mut v: Value) -> Value {
    sort_json_mut(&mut v);
    v
}

#[tokio::test]
async fn get_function() {
    let _ = tracing_subscriber::fmt::try_init();

    let mocks = SourceMocks::new();

    let fourbyte_response = serde_json::json!({"count":4,"next":null,"previous":null,"results":[{"id":844293,"created_at":"2022-08-26T12:22:13.363345Z","text_signature":"watch_tg_invmru_119a5a98(address,uint256,uint256)","hex_signature":"0x70a08231","bytes_signature":"p 1"},{"id":166551,"created_at":"2019-09-24T11:36:57.296021Z","text_signature":"passphrase_calculate_transfer(uint64,address)","hex_signature":"0x70a08231","bytes_signature":"p 1"},{"id":166550,"created_at":"2019-09-24T11:36:37.525020Z","text_signature":"branch_passphrase_public(uint256,bytes8)","hex_signature":"0x70a08231","bytes_signature":"p 1"},{"id":143,"created_at":"2016-07-09T03:58:27.545013Z","text_signature":"balanceOf(address)","hex_signature":"0x70a08231","bytes_signature":"p 1"}]});
    mocks.fourbyte_mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/api/v1/signatures/")
            .query_param("hex_signature", "70a08231");
        then.status(200)
            .header("Content-type", "application/json")
            .json_body(fourbyte_response);
    });

    let sigeth_response = serde_json::json!({"ok":true,"result":{"event":{},"function":{"0x70a08231":[{"name":"passphrase_calculate_transfer(uint64,address)","filtered":true},{"name":"branch_passphrase_public(uint256,bytes8)","filtered":true},{"name":"balanceOf(address)","filtered":false}]}}});
    mocks.sigeth_mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/api/v1/signatures")
            .query_param("function", "0x70a08231")
            .query_param_exists("all");
        then.status(200)
            .header("Content-type", "application/json")
            .json_body(sigeth_response);
    });

    let base = run_server(&mocks).await;

    let route = "/api/v1/abi/function?txInput=0x70a0823100000000000000000000000000000000219ab540356cbb839cbe05303d7705fa";
    let response: serde_json::Value = test_server::send_get_request(&base, route).await;

    mocks.assert();

    assert_eq!(
        sort_json(
            serde_json::json!([{"inputs":[{"components":[],"indexed":null,"name":"arg0","type":"address","value":"00000000219ab540356cbb839cbe05303d7705fa"}],"name":"balanceOf"}])
        ),
        sort_json(response)
    );
}

#[tokio::test]
async fn get_event() {
    let _ = tracing_subscriber::fmt::try_init();

    let mocks = SourceMocks::new();

    let fourbyte_response = serde_json::json!({"count":1,"next":null,"previous":null,"results":[{"id":1,"created_at":"2020-11-30T22:38:00.801049Z","text_signature":"Transfer(address,address,uint256)","hex_signature":"0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef","bytes_signature":"\u{1234}\u{4132}\u{1244}\u{1110}"}]});
    mocks.fourbyte_mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/api/v1/event-signatures/")
            .query_param(
                "hex_signature",
                "ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef",
            );
        then.status(200)
            .header("Content-type", "application/json")
            .json_body(fourbyte_response);
    });

    let sigeth_response = serde_json::json!({"ok":true,"result":{"event":{"0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef":[{"name":"Transfer(address,address,uint256)","filtered":false}]},"function":{}}});
    mocks.sigeth_mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/api/v1/signatures")
            .query_param(
                "event",
                "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef",
            )
            .query_param_exists("all");
        then.status(200)
            .header("Content-type", "application/json")
            .json_body(sigeth_response);
    });

    let eth_bytecode_db_request = serde_json::json!({"selector": "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"});
    let eth_bytecode_db_response = serde_json::json!({"eventDescriptions":[{"type":"event","name":"Transfer","inputs":"[{\"indexed\":true,\"internalType\":\"address\",\"name\":\"from\",\"type\":\"address\"},{\"indexed\":true,\"internalType\":\"address\",\"name\":\"to\",\"type\":\"address\"},{\"indexed\":false,\"internalType\":\"uint256\",\"name\":\"amount\",\"type\":\"uint256\"}]"}]});
    mocks.eth_bytecode_db_mock(|when, then| {
        when.method(httpmock::Method::POST)
            .path("/api/v2/event-descriptions:search")
            .header("Content-type", "application/json")
            .json_body(eth_bytecode_db_request);
        then.status(200)
            .header("Content-type", "application/json")
            .json_body(eth_bytecode_db_response);
    });

    let base = run_server(&mocks).await;

    let route = "/api/v1/abi/event?topics=0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef,000000000000000000000000b8ace4d9bc469ddc8e788e636e817c299a1a8150,000000000000000000000000f76c5b19e86c256482f4aad1dae620a0c3ac0cd6&data=00000000000000000000000000000000000000000000000000000000006acfc0";
    let response: serde_json::Value = test_server::send_get_request(&base, route).await;

    mocks.assert();

    assert_eq!(
        serde_json::json!([
            {
                "inputs":[
                    {"components":[],"indexed":true,"name":"from","type":"address","value":"b8ace4d9bc469ddc8e788e636e817c299a1a8150"},
                    {"components":[],"indexed":true,"name":"to","type":"address","value":"f76c5b19e86c256482f4aad1dae620a0c3ac0cd6"},
                    {"components":[],"indexed":false,"name":"amount","type":"uint256","value":"6acfc0"}],
                "name":"Transfer"
            },
            {
                "inputs":[
                    {"components":[],"indexed":true,"name":"arg0","type":"address","value":"b8ace4d9bc469ddc8e788e636e817c299a1a8150"},
                    {"components":[],"indexed":true,"name":"arg1","type":"address","value":"f76c5b19e86c256482f4aad1dae620a0c3ac0cd6"},
                    {"components":[],"indexed":false,"name":"arg2","type":"uint256","value":"6acfc0"}],
                "name":"Transfer"
            }
        ]),
        response,
    );
}

#[tokio::test]
async fn batch_get_events() {
    let _ = tracing_subscriber::fmt::try_init();

    let mocks = SourceMocks::new();

    {
        let eth_bytecode_db_request = serde_json::json!({"selectors":[
            "0x6e9ed8cf1494d7312e7618c9411ab219f27a9840ed74dda9581992ca7575eb86",
            "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef",
            "0x83f86eb20c894914ecf65cefc94682009cdb5066a609e8428699fa87b19b5c57",
        ]});
        let eth_bytecode_db_response = serde_json::json!({"responses": [
            {"eventDescriptions":[{"type":"event","name":"C","inputs":"[{\"indexed\":false,\"internalType\":\"uint256\",\"name\":\"c\",\"type\":\"uint256\"}]"}]},
            {"eventDescriptions":[]},
            {"eventDescriptions":[{"type":"event","name":"A","inputs":"[{\"indexed\":true,\"internalType\":\"uint256\",\"name\":\"a\",\"type\":\"uint256\"},{\"indexed\":true,\"internalType\":\"uint256\",\"name\":\"b\",\"type\":\"uint256\"}]"},{"type":"event","name":"A","inputs":"[{\"indexed\":true,\"internalType\":\"uint256\",\"name\":\"a2\",\"type\":\"uint256\"},{\"indexed\":true,\"internalType\":\"uint256\",\"name\":\"b2\",\"type\":\"uint256\"}]"}]},
        ]});
        mocks.eth_bytecode_db_mock(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/api/v2/event-descriptions:batch-search")
                .header("Content-type", "application/json")
                .json_body(eth_bytecode_db_request);
            then.status(200)
                .header("Content-type", "application/json")
                .json_body(eth_bytecode_db_response);
        });
    }

    let base = run_server(&mocks).await;

    let route = "/api/v1/abi/events:batch-get";
    let request = serde_json::json!({"requests":[
        {"data":"0x0000000000000000000000000000000000000000000000000000000000000001","topics":"0x6e9ed8cf1494d7312e7618c9411ab219f27a9840ed74dda9581992ca7575eb86"},
        {"data":"0x00000000000000000000000000000c000000000000000000000000000006acfc","topics":"0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef,000000000000000000000000b8ace4d9bc469ddc8e788e636e817c299a1a8150,000000000000000000000000f76c5b19e86c256482f4aad1dae620a0c3ac0cd6"},
        {"data":"0x","topics":"0x83f86eb20c894914ecf65cefc94682009cdb5066a609e8428699fa87b19b5c57,0x0000000000000000000000000000000000000000000000000000000000000001,0x0000000000000000000000000000000000000000000000000000000000000002"}
    ]});
    let response: serde_json::Value = test_server::send_post_request(&base, route, &request).await;

    mocks.assert();

    assert_eq!(
        serde_json::json!([
            {"abi": [{"inputs":[{"components":[],"indexed":false,"name":"c","type":"uint256","value":"1"}],"name":"C"}] },
            {"abi": [] },
            {"abi": [
                {"inputs":[{"components":[],"indexed":true,"name":"a","type":"uint256","value":"1"},{"components":[],"indexed":true,"name":"b","type":"uint256","value":"2"}],"name":"A"},
                {"inputs":[{"components":[],"indexed":true,"name":"a2","type":"uint256","value":"1"},{"components":[],"indexed":true,"name":"b2","type":"uint256","value":"2"}],"name":"A"},
            ]},
        ]),
        response,
    );
}
