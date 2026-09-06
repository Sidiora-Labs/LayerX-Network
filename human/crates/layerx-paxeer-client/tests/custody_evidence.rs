use std::path::Path;
use std::process::Command;

#[test]
fn real_weth_protocol_three_custody_evidence() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap_or_else(|| panic!("repository root"));
    let output = root
        .join("qual-logs")
        .join(format!("custody-evidence-{}", std::process::id()));
    let status = Command::new("python3")
        .arg(root.join("tests/bridge/local_credit_evidence.py"))
        .arg("--output")
        .arg(&output)
        .status()
        .unwrap_or_else(|error| panic!("custody evidence producer: {error}"));
    assert!(status.success(), "real custody evidence failed: {status}");
    let read = |name| std::fs::read(output.join(name)).unwrap_or_else(|error| panic!("{error}"));
    let profile = read("custody.profile");
    let credit = read("custody.credit");
    assert_eq!(profile.len(), 207);
    assert_eq!(credit.len(), 427);
    let identity =
        String::from_utf8(read("identity.json")).unwrap_or_else(|error| panic!("{error}"));
    let identity =
        layerx_paxeer_client::parse_json(&identity).unwrap_or_else(|error| panic!("{error:?}"));
    let did = identity
        .member("did")
        .and_then(layerx_paxeer_client::Json::as_text)
        .unwrap_or_else(|| panic!("actor DID"));
    let account = layerx_types::account::AccountId::parse(&format!("agent:{did}:main"))
        .unwrap_or_else(|error| panic!("{error:?}"));
    let beneficiary = layerx_paxeer_client::account_address_for_protocol(&account, 3)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(&credit[107..139], beneficiary.as_slice());
    let public: [u8; 32] = profile[65..97]
        .try_into()
        .unwrap_or_else(|error| panic!("{error}"));
    let authority =
        ed25519_dalek::VerifyingKey::from_bytes(&public).unwrap_or_else(|error| panic!("{error}"));
    let signature = ed25519_dalek::Signature::from_slice(&credit[363..])
        .unwrap_or_else(|error| panic!("{error}"));
    let mut message = b"LX:CUSTODY:CREDIT:v1".to_vec();
    message.extend_from_slice(&credit[..363]);
    authority
        .verify_strict(&message, &signature)
        .unwrap_or_else(|error| panic!("{error}"));
    message[20] ^= 1;
    assert!(authority.verify_strict(&message, &signature).is_err());
}
