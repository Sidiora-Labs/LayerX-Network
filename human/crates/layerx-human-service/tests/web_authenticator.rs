use std::fmt::Debug;
use std::io::{BufRead as _, BufReader, Write as _};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use layerx_human_service::auth::{AccountIdentity, AuthConfig, Passkeys, RateLimit};
use layerx_human_service::store::PrincipalId;
use layerx_human_test_support::{directory, install_and_open, retention_uniform, tenancy};
use serde_json::{json, Value};

const PRINCIPAL: &str = "act_00112233445566778899aabbccddeeff";
const ORIGIN: &str = "https://id.layerx.example";

fn required<T, E: Debug>(value: Result<T, E>) -> T {
    value.unwrap_or_else(|error| panic!("real browser authenticator: {error:?}"))
}

struct Authenticator {
    process: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Authenticator {
    fn open() -> Self {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../apps/web/e2e/software-authenticator.ts");
        let mut process = required(
            Command::new("node")
                .arg(script)
                .arg(ORIGIN)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn(),
        );
        let input = process
            .stdin
            .take()
            .unwrap_or_else(|| panic!("authenticator input"));
        let output = BufReader::new(
            process
                .stdout
                .take()
                .unwrap_or_else(|| panic!("authenticator output")),
        );
        Self {
            process,
            input,
            output,
        }
    }

    fn credential(&mut self, operation: &str, ceremony: &str) -> String {
        required(writeln!(
            self.input,
            "{}",
            json!({ "operation": operation, "ceremony": ceremony })
        ));
        required(self.input.flush());
        let mut response = String::new();
        assert!(required(self.output.read_line(&mut response)) > 0);
        assert!(response.len() <= 16_384);
        let response: Value = required(serde_json::from_str(&response));
        response["credential"]
            .as_str()
            .unwrap_or_else(|| panic!("authenticator credential"))
            .to_owned()
    }
}

impl Drop for Authenticator {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

#[test]
fn browser_authenticator_interoperates_with_real_passkeys_and_refuses_forgery_and_replay() {
    let root = directory("web-authenticator");
    let map = tenancy(&[(PRINCIPAL, "web-authenticator-tenant")]);
    let (mut store, _) = install_and_open(&root, &map, retention_uniform(86_400));
    let principal = required(PrincipalId::new(PRINCIPAL));
    let mut scope = required(store.principal(&principal));
    let passkeys = required(Passkeys::new(AuthConfig {
        rp_id: "id.layerx.example".to_owned(),
        rp_name: "LayerX".to_owned(),
        origin: ORIGIN.to_owned(),
        ceremony_ttl_secs: 300,
        assertion_ttl_secs: 60,
        session_ttl_secs: 300,
        refresh_ttl_secs: 3_600,
        step_up_ttl_secs: 60,
        rate_limit: RateLimit {
            attempts: 100,
            window_secs: 60,
        },
    }));
    let account = required(AccountIdentity::new(PRINCIPAL, "Browser qualification"));
    let mut authenticator = Authenticator::open();
    let registration =
        required(passkeys.begin_registration(&mut scope, &account, "Primary passkey", 100));
    let credential = authenticator.credential("register", &registration.ceremony);
    let registered = required(passkeys.finish_registration(
        &mut scope,
        &registration.registration_id,
        &credential,
        101,
    ));
    let assertion = required(passkeys.begin_assertion(&mut scope, 102));
    let credential = authenticator.credential("assert", &assertion.ceremony);
    let mut altered: Value = required(serde_json::from_slice(&required(
        URL_SAFE_NO_PAD.decode(&credential),
    )));
    let mut signature = required(
        URL_SAFE_NO_PAD.decode(
            altered["signature"]
                .as_str()
                .unwrap_or_else(|| panic!("signature")),
        ),
    );
    signature[0] ^= 1;
    altered["signature"] = Value::String(URL_SAFE_NO_PAD.encode(signature));
    let altered = URL_SAFE_NO_PAD.encode(required(serde_json::to_vec(&altered)));
    assert!(passkeys
        .finish_assertion(&mut scope, &assertion.assertion_id, &altered, 103)
        .is_err());
    let verified =
        required(passkeys.finish_assertion(&mut scope, &assertion.assertion_id, &credential, 103));
    assert_eq!(verified.passkey_id, registered.passkey_id);
    assert!(passkeys
        .finish_assertion(&mut scope, &assertion.assertion_id, &credential, 104)
        .is_err());
    let assertion = required(passkeys.begin_assertion(&mut scope, 105));
    let credential = authenticator.credential("assert", &assertion.ceremony);
    let verified =
        required(passkeys.finish_assertion(&mut scope, &assertion.assertion_id, &credential, 106));
    assert_eq!(verified.passkey_id, registered.passkey_id);
}
