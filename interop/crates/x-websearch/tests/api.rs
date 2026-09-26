use std::error::Error;
use std::io::{Read, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use k256::ecdsa::SigningKey;
use k256::PublicKey;
use native_tls::{Certificate, Identity, TlsAcceptor};
use openssl::asn1::Asn1Time;
use openssl::bn::{BigNum, MsbOption};
use openssl::ec::{EcGroup, EcKey};
use openssl::hash::MessageDigest;
use openssl::nid::Nid;
use openssl::pkey::PKey;
use openssl::x509::extension::{
    BasicConstraints, ExtendedKeyUsage, KeyUsage, SubjectAlternativeName,
};
use openssl::x509::{X509Builder, X509NameBuilder};
use serde_json::{json, Value};
use x_websearch::api::{
    self, envelope_aad, envelope_key, envelope_shared_x, open_envelope, seal_envelope_with,
    ApiClient, ApiError, ApiHeader, ApiPayload, Credential, Envelope, ANSWER_MEDIA_TYPE, KIND_API,
    LEVEL_MAJORITY, LEVEL_SINGLE, METHOD_GET, METHOD_POST,
};
use x_websearch::attest::{
    recover_signer, sign_digest, signer_address, AttestError, Attestor, AttestorSet, Level, Ready,
    SignatureExchange,
};
use x_websearch::canonical::content_digest;
use x_websearch::config::FetchLimits;
use x_websearch::content::ContentStore;
use x_websearch::fetch::Fetcher;
use x_websearch::index::WebIndex;
use x_websearch::submit::{fulfil_calldata, Outcome, Submitter};
use x_websearch::watch::{hex0x, keccak, unhex0x, EvmRpc, WebRequest};

type Checked<T = ()> = Result<T, Box<dyn Error>>;

const CHAIN_ID: u64 = 713_714;

/// The credentials the loopback API accepts, one per test attestor. They
/// exist only in this test's memory and in the envelopes sealed to the
/// attestors.
const CREDENTIAL_ONE: &str = "loopback-credential-one-4f1c9e27b3";
const CREDENTIAL_TWO: &str = "loopback-credential-two-8d2a6b51c0";
const CREDENTIAL_HEADER: &str = "X-Api-Key";

fn fail(message: impl Into<String>) -> Box<dyn Error> {
    message.into().into()
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/api")
}

fn testdata(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../modules/xweb/types/testdata")
        .join(name)
}

fn read_json(path: &Path) -> Checked<Value> {
    Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
}

fn text<'a>(value: &'a Value, pointer: &str) -> Checked<&'a str> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| fail(format!("missing {pointer}")))
}

fn bytes(value: &Value, pointer: &str) -> Checked<Vec<u8>> {
    unhex0x(text(value, pointer)?).ok_or_else(|| fail(format!("{pointer} is not hex")))
}

fn fixed<const N: usize>(value: &Value, pointer: &str) -> Checked<[u8; N]> {
    bytes(value, pointer)?
        .try_into()
        .map_err(|_| fail(format!("{pointer} is not {N} bytes")))
}

fn array<'a>(value: &'a Value, pointer: &str) -> Checked<&'a Vec<Value>> {
    value
        .pointer(pointer)
        .and_then(Value::as_array)
        .ok_or_else(|| fail(format!("missing {pointer}")))
}

fn labelled_key(label: &str) -> Checked<SigningKey> {
    Ok(SigningKey::from_slice(&keccak(label.as_bytes()))?)
}

/// The test attestor key `i`: 0x3c, zeros, then `i`.
fn attestor_key(index: u8) -> Checked<SigningKey> {
    let mut secret = [0_u8; 32];
    secret[0] = 0x3c;
    secret[31] = index;
    Ok(SigningKey::from_slice(&secret)?)
}

/// A one-use ephemeral key for sealing a test envelope.
fn ephemeral_key(index: u8) -> Checked<SigningKey> {
    let mut secret = [0_u8; 32];
    secret[0] = 0x5e;
    secret[31] = index;
    Ok(SigningKey::from_slice(&secret)?)
}

fn headers_of(value: &Value) -> Checked<Vec<ApiHeader>> {
    value
        .as_array()
        .ok_or_else(|| fail("headers are not a list"))?
        .iter()
        .map(|header| {
            Ok(ApiHeader::new(
                text(header, "/name")?,
                text(header, "/value")?,
            ))
        })
        .collect()
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Checked<Self> {
        let path =
            std::env::temp_dir().join(format!("x-websearch-api-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn read_request(stream: &mut impl Read) -> Checked<(String, Vec<u8>)> {
    let mut data = Vec::new();
    let mut chunk = [0; 4_096];
    loop {
        if let Some(end) = data.windows(4).position(|window| window == b"\r\n\r\n") {
            let head = String::from_utf8(data[..end].to_vec())?;
            let length = head
                .split("\r\n")
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            if data.len() >= end + 4 + length {
                return Ok((head, data[end + 4..end + 4 + length].to_vec()));
            }
        }
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Err(fail("connection closed early"));
        }
        data.extend_from_slice(&chunk[..read]);
    }
}

/// A self-signed certificate for the loopback address and its identity.
fn loopback_certificate() -> Checked<(Identity, Certificate)> {
    let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1)?;
    let key = PKey::from_ec_key(EcKey::generate(&group)?)?;
    let mut name = X509NameBuilder::new()?;
    name.append_entry_by_text("CN", "127.0.0.1")?;
    let name = name.build();
    let mut builder = X509Builder::new()?;
    builder.set_version(2)?;
    let mut serial = BigNum::new()?;
    serial.rand(64, MsbOption::MAYBE_ZERO, false)?;
    builder.set_serial_number(serial.to_asn1_integer()?.as_ref())?;
    builder.set_subject_name(&name)?;
    builder.set_issuer_name(&name)?;
    builder.set_pubkey(&key)?;
    builder.set_not_before(Asn1Time::days_from_now(0)?.as_ref())?;
    builder.set_not_after(Asn1Time::days_from_now(1)?.as_ref())?;
    builder.append_extension(BasicConstraints::new().critical().ca().build()?)?;
    builder.append_extension(
        KeyUsage::new()
            .critical()
            .digital_signature()
            .key_cert_sign()
            .build()?,
    )?;
    builder.append_extension(ExtendedKeyUsage::new().server_auth().build()?)?;
    let san = SubjectAlternativeName::new()
        .ip("127.0.0.1")
        .build(&builder.x509v3_context(None, None))?;
    builder.append_extension(san)?;
    builder.sign(&key, MessageDigest::sha256())?;
    let certificate = builder.build();
    let pem = certificate.to_pem()?;
    Ok((
        Identity::from_pkcs8(&pem, &key.private_key_to_pem_pkcs8()?)?,
        Certificate::from_pem(&pem)?,
    ))
}

/// One call the loopback API received.
#[derive(Clone, Debug)]
struct Call {
    method: String,
    target: String,
    credential: Option<String>,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    answered: Vec<u8>,
}

/// An https API on the loopback interface. Every call without a known
/// credential header is answered 401. `/quote` answers the fixture's JSON
/// with a call counter and a per-call nonce the pointers do not select,
/// `/raw` a plain text body and `/html` a page that is not JSON.
struct ApiServer {
    address: SocketAddr,
    root: Certificate,
    calls: Arc<Mutex<Vec<Call>>>,
}

fn respond(head: &str, body: Vec<u8>, quote: &Value, counter: &AtomicU64) -> (Call, Vec<u8>) {
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next().unwrap_or_default().split(' ');
    let method = request_line.next().unwrap_or_default().to_owned();
    let target = request_line.next().unwrap_or_default().to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let credential = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(CREDENTIAL_HEADER))
        .map(|(_, value)| value.clone());
    let known = matches!(credential.as_deref(), Some(CREDENTIAL_ONE | CREDENTIAL_TWO));
    let path = target.split('?').next().unwrap_or_default();
    let (status, media, answered) = if known && path == "/quote" {
        let count = counter.fetch_add(1, Ordering::SeqCst);
        let mut answer = quote.clone();
        if let Some(object) = answer.as_object_mut() {
            object.insert("served".to_owned(), json!(count));
            object.insert(
                "nonce".to_owned(),
                json!(hex0x(&keccak(&count.to_be_bytes()))),
            );
            object.insert("method".to_owned(), json!(method));
        }
        (
            "200 OK",
            "application/json; charset=utf-8",
            answer.to_string().into_bytes(),
        )
    } else if known && path == "/raw" {
        (
            "200 OK",
            "text/plain; charset=utf-8",
            b"status: operational\n".to_vec(),
        )
    } else if known && path == "/html" {
        (
            "200 OK",
            "text/html",
            b"<html><body>quote</body></html>".to_vec(),
        )
    } else if known && path == "/moved" {
        ("302 Found", "text/plain", b"moved".to_vec())
    } else {
        ("404 Not Found", "text/plain", Vec::new())
    };
    let mut response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {media}\r\nContent-Length: {}\r\nLocation: /quote\r\nConnection: close\r\n\r\n",
        answered.len()
    )
    .into_bytes();
    response.extend_from_slice(&answered);
    let call = Call {
        method,
        target,
        credential,
        headers,
        body,
        answered,
    };
    (call, response)
}

impl ApiServer {
    fn start() -> Checked<Self> {
        let (identity, root) = loopback_certificate()?;
        let acceptor = TlsAcceptor::new(identity)?;
        let quote = read_json(&fixtures().join("quote.json"))?;
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let calls = Arc::new(Mutex::new(Vec::new()));
        let served = Arc::clone(&calls);
        let counter = AtomicU64::new(0);
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let Ok(mut tls) = acceptor.accept(stream) else {
                    continue;
                };
                let Ok((head, body)) = read_request(&mut tls) else {
                    continue;
                };
                let (call, response) = respond(&head, body, &quote, &counter);
                served
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(call);
                let _ = tls.write_all(&response);
                let _ = tls.shutdown();
            }
        });
        Ok(Self {
            address,
            root,
            calls,
        })
    }

    fn url(&self, target: &str) -> String {
        format!("https://{}{target}", self.address)
    }

    fn origin(&self) -> String {
        format!("https://{}", self.address)
    }

    fn calls(&self) -> Vec<Call> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

fn loopback_fetcher() -> Checked<Fetcher> {
    Ok(Fetcher::new(FetchLimits {
        connect_timeout_ms: 3_000,
        total_timeout_ms: 10_000,
        max_body_bytes: 2_097_152,
        max_redirects: 3,
        allow_loopback: true,
    })?)
}

/// One sidecar: its attestor over its own data directory, its signature
/// exchange and its submitter journal, all under `home`.
struct Sidecar {
    home: PathBuf,
    key: SigningKey,
    attestor: Attestor,
    store: Arc<ContentStore>,
    exchange: SignatureExchange,
}

impl Sidecar {
    fn open(home: PathBuf, index: u8, server: &ApiServer) -> Checked<Self> {
        let data = home.join("data");
        let store = Arc::new(ContentStore::open(&data, &[])?);
        let web_index = Arc::new(WebIndex::open(&data)?);
        let key = attestor_key(index)?;
        let attestor = Attestor::new(
            key.clone(),
            CHAIN_ID,
            Arc::new(loopback_fetcher()?),
            web_index,
            Arc::clone(&store),
        )
        .with_api_roots(vec![server.root.clone()]);
        let exchange = SignatureExchange::open(&home.join("state"), &[])?;
        Ok(Self {
            home,
            key,
            attestor,
            store,
            exchange,
        })
    }

    fn address(&self) -> [u8; 20] {
        signer_address(&self.key)
    }
}

/// An envelope sealing `credential` to an attestor for an origin.
fn envelope(
    attestor: &SigningKey,
    ephemeral: u8,
    origin: &str,
    credential: &[ApiHeader],
) -> Checked<Envelope> {
    let plaintext = Credential::encode(credential)?;
    let sealed = seal_envelope_with(
        &PublicKey::from(attestor.verifying_key()),
        &ephemeral_key(ephemeral)?,
        [ephemeral; 12],
        origin,
        &plaintext,
    )?;
    Ok(Envelope::parse(&sealed)?)
}

fn credential(value: &str) -> Vec<ApiHeader> {
    vec![ApiHeader::new(CREDENTIAL_HEADER, value)]
}

fn request(request_id: u64, payload: &ApiPayload) -> Checked<WebRequest> {
    let mut requester = [0_u8; 20];
    requester[17..].copy_from_slice(&[0x0a, 0x11, 0xce]);
    Ok(WebRequest {
        request_id,
        requester,
        kind: KIND_API,
        payload: payload.encode()?,
        callback_gas: 200_000,
        paid: [0; 32],
        timeout_height: 528,
        block_number: 0x1c,
    })
}

fn quote_payload(server: &ApiServer, envelopes: Vec<Envelope>) -> ApiPayload {
    ApiPayload {
        method: METHOD_GET,
        level: LEVEL_MAJORITY,
        attestor: [0; 20],
        url: server.url("/quote?asset=SID"),
        headers: vec![ApiHeader::new("Accept", "application/json")],
        body: Vec::new(),
        pointers: vec!["/quote/amount".to_owned(), "/decimals".to_owned()],
        envelopes,
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn files_under(directory: &Path, out: &mut Vec<PathBuf>) -> Checked {
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            files_under(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

/// Fails when a credential appears in any file under `home` or in any of
/// the `lines` the sidecar formats for its log.
fn assert_credentials_absent(home: &Path, lines: &[String]) -> Checked<usize> {
    let mut files = Vec::new();
    files_under(home, &mut files)?;
    for credential in [CREDENTIAL_ONE, CREDENTIAL_TWO] {
        for file in &files {
            if contains(&std::fs::read(file)?, credential.as_bytes()) {
                return Err(fail(format!("{} holds a credential", file.display())));
            }
        }
        for line in lines {
            if line.contains(credential) {
                return Err(fail(format!("a log line holds a credential: {line}")));
            }
        }
    }
    Ok(files.len())
}

#[test]
fn the_api_payload_codec_matches_every_pinned_vector_and_refusal() -> Checked {
    let vectors = read_json(&testdata("api-vectors.json"))?;
    let envelopes = read_json(&testdata("envelope-vectors.json"))?;
    let envelope_named = |name: &str| -> Checked<Envelope> {
        let vector = array(&envelopes, "/vectors")?
            .iter()
            .find(|vector| vector.get("name").and_then(Value::as_str) == Some(name))
            .ok_or_else(|| fail(format!("no envelope vector {name}")))?;
        Ok(Envelope::parse(&bytes(vector, "/envelope")?)?)
    };
    let list = array(&vectors, "/vectors")?;
    assert_eq!(list.len(), 4);
    for vector in list {
        let name = text(vector, "/name")?;
        let method = match text(vector, "/method")? {
            "GET" => METHOD_GET,
            "POST" => METHOD_POST,
            other => return Err(fail(format!("{name}: method {other}"))),
        };
        let level = vector
            .get("level")
            .and_then(Value::as_u64)
            .and_then(|level| u8::try_from(level).ok())
            .ok_or_else(|| fail(format!("{name}: level")))?;
        let built = ApiPayload {
            method,
            level,
            attestor: fixed(vector, "/attestor")?,
            url: text(vector, "/url")?.to_owned(),
            headers: headers_of(&vector["headers"])?,
            body: text(vector, "/body")?.as_bytes().to_vec(),
            pointers: array(vector, "/pointers")?
                .iter()
                .map(|pointer| pointer.as_str().map(str::to_owned))
                .collect::<Option<_>>()
                .ok_or_else(|| fail(format!("{name}: pointers")))?,
            envelopes: array(vector, "/envelopes")?
                .iter()
                .map(|envelope| envelope_named(envelope.as_str().unwrap_or_default()))
                .collect::<Checked<_>>()?,
        };
        let payload = bytes(vector, "/payload")?;
        assert_eq!(built.encode()?, payload, "{name}");
        let decoded = ApiPayload::decode(&payload)?;
        assert_eq!(decoded, built, "{name}");
        assert_eq!(decoded.origin()?, text(vector, "/origin")?, "{name}");
        assert_eq!(keccak(&payload), fixed(vector, "/payload_hash")?, "{name}");
        let expected = if level == LEVEL_SINGLE {
            Level::Single(built.attestor)
        } else {
            Level::Majority
        };
        assert_eq!(decoded.attestation_level(), expected, "{name}");
    }
    let refusals = array(&vectors, "/refusals")?;
    assert_eq!(refusals.len(), 21);
    for refusal in refusals {
        let name = text(refusal, "/name")?;
        let refuses = text(refusal, "/refuses")?;
        match ApiPayload::decode(&bytes(refusal, "/payload")?) {
            Err(ApiError::Payload(message)) if message.contains(refuses) => {}
            other => return Err(fail(format!("{name}: {other:?}, want {refuses}"))),
        }
    }
    Ok(())
}

#[test]
fn envelopes_open_and_seal_exactly_as_the_shared_vectors_pin() -> Checked {
    let envelopes = read_json(&testdata("envelope-vectors.json"))?;
    for vector in array(&envelopes, "/vectors")? {
        let name = text(vector, "/name")?;
        let attestor = labelled_key(text(vector, "/attestor_key_label")?)?;
        let ephemeral = labelled_key(text(vector, "/ephemeral_key_label")?)?;
        assert_eq!(
            attestor.verifying_key().to_encoded_point(true).as_bytes(),
            bytes(vector, "/attestor_public_key")?.as_slice(),
            "{name}"
        );
        assert_eq!(
            ephemeral.verifying_key().to_encoded_point(true).as_bytes(),
            bytes(vector, "/ephemeral_public_key")?.as_slice(),
            "{name}"
        );
        let address: [u8; 20] = fixed(vector, "/attestor")?;
        assert_eq!(signer_address(&attestor), address, "{name}");
        let origin = text(vector, "/origin")?;
        let shared = envelope_shared_x(
            attestor.as_nonzero_scalar(),
            &PublicKey::from(ephemeral.verifying_key()),
        )?;
        assert_eq!(shared.to_vec(), bytes(vector, "/shared_x")?, "{name}");
        let from_sender = envelope_shared_x(
            ephemeral.as_nonzero_scalar(),
            &PublicKey::from(attestor.verifying_key()),
        )?;
        assert_eq!(*shared, *from_sender, "{name}");
        let compressed = bytes(vector, "/ephemeral_public_key")?;
        let key = envelope_key(&shared[..], &compressed)?;
        assert_eq!(key.to_vec(), bytes(vector, "/aes_key")?, "{name}");
        assert_eq!(
            envelope_aad(&address, origin),
            bytes(vector, "/aad")?,
            "{name}"
        );
        let credential = headers_of(&vector["credential"])?;
        let plaintext = Credential::encode(&credential)?;
        assert_eq!(plaintext.to_vec(), bytes(vector, "/plaintext")?, "{name}");
        let raw = bytes(vector, "/envelope")?;
        let sealed = seal_envelope_with(
            &PublicKey::from(attestor.verifying_key()),
            &ephemeral,
            fixed(vector, "/nonce")?,
            origin,
            &plaintext,
        )?;
        assert_eq!(sealed, raw, "{name}");
        let parsed = Envelope::parse(&raw)?;
        assert_eq!(parsed.bytes(), raw, "{name}");
        assert_eq!(parsed.ciphertext, bytes(vector, "/ciphertext")?, "{name}");
        let opened = open_envelope(&raw, &attestor, origin)?;
        assert_eq!(opened.to_vec(), plaintext.to_vec(), "{name}");
        let decoded = Credential::decode(&opened, &[])?;
        let names: Vec<&str> = credential
            .iter()
            .map(|header| header.name.as_str())
            .collect();
        assert_eq!(decoded.names(), names, "{name}");
    }
    for refusal in array(&envelopes, "/refusals")? {
        let name = text(refusal, "/name")?;
        let key = labelled_key(text(refusal, "/open_with")?)?;
        let refuses = text(refusal, "/refuses")?;
        match open_envelope(
            &bytes(refusal, "/envelope")?,
            &key,
            text(refusal, "/origin")?,
        ) {
            Err(ApiError::Envelope(message)) if message.contains(refuses) => {}
            Err(other) => return Err(fail(format!("{name}: {other}, want {refuses}"))),
            Ok(_) => return Err(fail(format!("{name}: opened, want {refuses}"))),
        }
    }
    let public = vec![ApiHeader::new("x-api-key", "public")];
    let plaintext = Credential::encode(&credential(CREDENTIAL_ONE))?;
    match Credential::decode(&plaintext, &public) {
        Err(ApiError::Credential(message)) => {
            assert!(message.contains("repeats a public header"));
            assert!(!message.contains(CREDENTIAL_ONE));
        }
        Err(other) => return Err(fail(format!("repeated header: {other}"))),
        Ok(_) => return Err(fail("a credential repeating a public header decoded")),
    }
    match Credential::encode(&[ApiHeader::new("Host", CREDENTIAL_ONE)]) {
        Err(ApiError::Credential(message)) => {
            assert!(message.contains("sets itself"));
            assert!(!message.contains(CREDENTIAL_ONE));
        }
        other => {
            return Err(fail(format!(
                "restricted credential header: {:?}",
                other.map(|_| ())
            )))
        }
    }
    let broken = format!("{CREDENTIAL_ONE}\n");
    match Credential::encode(&credential(&broken)) {
        Err(ApiError::Credential(message)) => assert!(!message.contains(CREDENTIAL_ONE)),
        other => {
            return Err(fail(format!(
                "line break in a credential: {:?}",
                other.map(|_| ())
            )))
        }
    }
    Ok(())
}

#[test]
fn pointers_select_and_canonicalise_as_the_vectors_pin() -> Checked {
    let vectors = read_json(&fixtures().join("select-vectors.json"))?;
    for vector in array(&vectors, "/vectors")? {
        let name = text(vector, "/name")?;
        let pointers: Vec<String> = array(vector, "/pointers")?
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        let answer = api::select_answer(text(vector, "/body")?.as_bytes(), &pointers)?;
        assert_eq!(
            String::from_utf8(answer)?,
            text(vector, "/answer")?,
            "{name}"
        );
    }
    for refusal in array(&vectors, "/refusals")? {
        let name = text(refusal, "/name")?;
        let pointers: Vec<String> = array(refusal, "/pointers")?
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        match api::select_answer(text(refusal, "/body")?.as_bytes(), &pointers) {
            Err(error) if error.code() == text(refusal, "/refuses")? => {
                if let ApiError::Pointer(pointer) = &error {
                    assert!(pointers.contains(pointer), "{name}");
                    assert!(error.to_string().contains(pointer.as_str()), "{name}");
                }
            }
            other => return Err(fail(format!("{name}: {other:?}"))),
        }
    }
    let raw = b"not json at all \xff";
    assert_eq!(api::select_answer(raw, &[])?, raw.to_vec());
    Ok(())
}

#[test]
fn two_sidecars_holding_different_envelopes_sign_the_same_digest() -> Checked {
    let scratch = Scratch::new("majority")?;
    let server = ApiServer::start()?;
    let one = Sidecar::open(scratch.0.join("one"), 1, &server)?;
    let two = Sidecar::open(scratch.0.join("two"), 2, &server)?;
    let origin = server.origin();
    let payload = quote_payload(
        &server,
        vec![
            envelope(&one.key, 1, &origin, &credential(CREDENTIAL_ONE))?,
            envelope(&two.key, 2, &origin, &credential(CREDENTIAL_TWO))?,
        ],
    );
    let request = request(21, &payload)?;
    let first = one.attestor.attest(&request)?;
    let second = two.attestor.attest(&request)?;

    let calls = server.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].credential.as_deref(), Some(CREDENTIAL_ONE));
    assert_eq!(calls[1].credential.as_deref(), Some(CREDENTIAL_TWO));
    assert_ne!(calls[0].answered, calls[1].answered);
    for call in &calls {
        assert_eq!(call.method, "GET");
        assert_eq!(call.target, "/quote?asset=SID");
        assert!(call.body.is_empty());
        assert!(call
            .headers
            .iter()
            .any(|(name, value)| name == "accept" && value == "application/json"));
    }

    let answer = b"[\"3.114\",3]".to_vec();
    assert_eq!(first.response, answer);
    assert_eq!(second.response, answer);
    assert_eq!(first.level, Level::Majority);
    assert_eq!(second.level, Level::Majority);
    assert_eq!(
        first.attestation.content_digest,
        second.attestation.content_digest
    );
    assert_eq!(first.digest, second.digest);
    let canonical = api::api_canonical_bytes(&request.payload, ANSWER_MEDIA_TYPE, &answer)?;
    assert_eq!(first.attestation.content_digest, content_digest(&canonical));
    assert_eq!(first.attestation.kind, KIND_API);
    assert_eq!(first.attestation.payload_hash, keccak(&request.payload));
    assert_eq!(first.attestation.full_length, 11);
    assert_eq!(
        recover_signer(&first.digest, &first.signature)?,
        one.address()
    );
    assert_eq!(
        recover_signer(&second.digest, &second.signature)?,
        two.address()
    );
    assert_eq!(one.store.get(&first.attestation.content_digest)?, None);

    let set = AttestorSet {
        signers: vec![
            one.address(),
            two.address(),
            signer_address(&attestor_key(3)?),
        ],
        threshold: 2,
    };
    one.exchange.record(first.clone());
    assert_eq!(one.exchange.ready(21, &set), None);
    assert_eq!(
        one.exchange.accept("two", 21, &second.record(), &set),
        Ok(Some(two.address()))
    );
    let ready = one
        .exchange
        .ready(21, &set)
        .ok_or_else(|| fail("two agreeing signatures are not ready"))?;
    assert_eq!(ready.signatures.len(), 2);
    assert_eq!(ready.response, answer);

    let lines = vec![
        format!("{first:?}"),
        format!("{second:?}"),
        first.record().to_string(),
        second.record().to_string(),
        format!("{payload:?}"),
        format!("{ready:?}"),
    ];
    assert!(assert_credentials_absent(&scratch.0, &lines)? > 0);
    Ok(())
}

#[test]
fn a_request_without_an_envelope_for_this_attestor_is_refused_before_any_call() -> Checked {
    let scratch = Scratch::new("refusals")?;
    let server = ApiServer::start()?;
    let one = Sidecar::open(scratch.0.join("one"), 1, &server)?;
    let two = Sidecar::open(scratch.0.join("two"), 2, &server)?;
    let origin = server.origin();
    let mut lines = Vec::new();

    let only_two = quote_payload(
        &server,
        vec![envelope(&two.key, 2, &origin, &credential(CREDENTIAL_TWO))?],
    );
    let refused = one.attestor.attest(&request(31, &only_two)?);
    assert_eq!(refused, Err(AttestError::Api(ApiError::NoEnvelope)));
    lines.push(format!("{refused:?}"));
    assert!(server.calls().is_empty());

    let elsewhere = quote_payload(
        &server,
        vec![envelope(
            &one.key,
            1,
            "https://127.0.0.1:1",
            &credential(CREDENTIAL_ONE),
        )?],
    );
    let refused = one.attestor.attest(&request(32, &elsewhere)?);
    match &refused {
        Err(AttestError::Api(ApiError::Envelope(message))) => {
            assert!(message.contains("does not authenticate"));
        }
        other => return Err(fail(format!("an envelope for another origin: {other:?}"))),
    }
    lines.push(format!("{refused:?}"));
    if let Err(error) = &refused {
        lines.push(error.to_string());
    }
    assert!(server.calls().is_empty());

    let clashing = ApiPayload {
        headers: vec![ApiHeader::new("x-api-key", "public")],
        ..quote_payload(
            &server,
            vec![envelope(&one.key, 3, &origin, &credential(CREDENTIAL_ONE))?],
        )
    };
    let refused = one.attestor.attest(&request(33, &clashing)?);
    assert!(matches!(
        &refused,
        Err(AttestError::Api(ApiError::Credential(message))) if message.contains("repeats a public header")
    ));
    lines.push(format!("{refused:?}"));
    assert!(server.calls().is_empty());

    let bare = quote_payload(&server, Vec::new());
    let refused = one.attestor.attest(&request(34, &bare)?);
    assert_eq!(refused, Err(AttestError::Api(ApiError::Status(401))));
    let calls = server.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].credential, None);

    let moved = ApiPayload {
        url: server.url("/moved"),
        ..quote_payload(
            &server,
            vec![envelope(&one.key, 4, &origin, &credential(CREDENTIAL_ONE))?],
        )
    };
    let refused = one.attestor.attest(&request(35, &moved)?);
    assert_eq!(refused, Err(AttestError::Api(ApiError::Status(302))));
    assert_eq!(server.calls().len(), 2);

    assert_credentials_absent(&scratch.0, &lines)?;
    Ok(())
}

#[test]
fn a_call_answers_the_raw_body_or_names_what_it_refuses() -> Checked {
    let scratch = Scratch::new("answers")?;
    let server = ApiServer::start()?;
    let one = Sidecar::open(scratch.0.join("one"), 1, &server)?;
    let origin = server.origin();
    let mut lines = Vec::new();
    let client = ApiClient::new(Arc::new(loopback_fetcher()?), &[server.root.clone()])?;
    let raw = ApiPayload {
        url: server.url("/raw"),
        pointers: Vec::new(),
        ..quote_payload(
            &server,
            vec![envelope(&one.key, 5, &origin, &credential(CREDENTIAL_ONE))?],
        )
    };
    let raw_bytes = raw.encode()?;
    let answered = api::answer(&client, &one.key, &raw_bytes, &raw)?;
    assert_eq!(answered.answer, b"status: operational\n".to_vec());
    assert_eq!(answered.media_type, "text/plain");
    assert_eq!(
        answered.canonical,
        api::api_canonical_bytes(&raw_bytes, "text/plain", b"status: operational\n")?
    );
    assert_eq!(answered.digest, content_digest(&answered.canonical));
    lines.push(format!("{answered:?}"));

    let html = ApiPayload {
        url: server.url("/html"),
        ..raw.clone()
    };
    let refused = api::answer(&client, &one.key, &html.encode()?, &html);
    assert!(matches!(refused, Err(ApiError::NotJson(_))));
    let missing = ApiPayload {
        pointers: vec!["/quote/amount".to_owned(), "/quote/missing".to_owned()],
        ..quote_payload(
            &server,
            vec![envelope(&one.key, 6, &origin, &credential(CREDENTIAL_ONE))?],
        )
    };
    let refused = api::answer(&client, &one.key, &missing.encode()?, &missing);
    assert_eq!(refused, Err(ApiError::Pointer("/quote/missing".to_owned())));
    if let Err(error) = &refused {
        assert!(error.to_string().contains("/quote/missing"));
        lines.push(error.to_string());
    }

    let strict = ApiClient::new(
        Arc::new(Fetcher::new(FetchLimits {
            connect_timeout_ms: 3_000,
            total_timeout_ms: 10_000,
            max_body_bytes: 2_097_152,
            max_redirects: 3,
            allow_loopback: false,
        })?),
        &[server.root.clone()],
    )?;
    let before = server.calls().len();
    let refused = api::answer(&strict, &one.key, &raw_bytes, &raw);
    assert!(matches!(refused, Err(ApiError::Fetch(_))));
    let untrusted = ApiClient::new(Arc::new(loopback_fetcher()?), &[])?;
    let refused = api::answer(&untrusted, &one.key, &raw_bytes, &raw);
    assert!(matches!(refused, Err(ApiError::Fetch(_))));
    assert_eq!(server.calls().len(), before);

    assert_credentials_absent(&scratch.0, &lines)?;
    Ok(())
}

/// A loopback EVM endpoint answering from `tests/fixtures/api/evm.json`
/// and returning the hash of every raw transaction it is sent.
struct Evm {
    address: SocketAddr,
    sent: Arc<Mutex<Vec<Vec<u8>>>>,
}

fn serve_evm(stream: &mut TcpStream, answers: &Value, sent: &Mutex<Vec<Vec<u8>>>) -> Checked {
    let (_, body) = read_request(stream)?;
    let request: Value = serde_json::from_slice(&body)?;
    let method = text(&request, "/method")?;
    let result = if method == "eth_sendRawTransaction" {
        let raw = bytes(&request, "/params/0")?;
        let hash = hex0x(&keccak(&raw));
        sent.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(raw);
        json!(hash)
    } else {
        answers
            .get(method)
            .cloned()
            .ok_or_else(|| fail(format!("no answer for {method}")))?
    };
    let reply = json!({
        "jsonrpc": "2.0",
        "id": request.get("id").cloned().unwrap_or(json!(1)),
        "result": result,
    })
    .to_string();
    stream.write_all(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
            reply.len()
        )
        .as_bytes(),
    )?;
    Ok(())
}

impl Evm {
    fn start() -> Checked<Self> {
        let answers = read_json(&fixtures().join("evm.json"))?;
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let sent = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&sent);
        thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let _ = serve_evm(&mut stream, &answers, &recorded);
            }
        });
        Ok(Self { address, sent })
    }
}

/// Posts fulfil for `ready` against the loopback EVM endpoint and checks the
/// one transaction carries `kept` and not `dropped`.
fn submit_one(home: &Path, ready: &Ready, kept: &[u8; 65], dropped: &[u8; 65]) -> Checked<Outcome> {
    let evm = Evm::start()?;
    let submitter = Submitter::open(
        EvmRpc::new(&format!("http://{}/", evm.address))?,
        SigningKey::from_slice(&[0x77; 32])?,
        CHAIN_ID,
        &home.join("journal"),
    )?;
    let outcome = submitter.submit(ready)?;
    let sent = evm
        .sent
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        outcome,
        Outcome::Sent {
            hash: keccak(&sent[0])
        }
    );
    let calldata = fulfil_calldata(ready);
    assert!(contains(&sent[0], &calldata));
    assert!(contains(&calldata, kept));
    assert!(!contains(&calldata, dropped));
    Ok(outcome)
}

#[test]
fn under_the_single_level_only_the_named_attestor_signs_and_fulfil_carries_one_signature() -> Checked
{
    let scratch = Scratch::new("single")?;
    let server = ApiServer::start()?;
    let one = Sidecar::open(scratch.0.join("one"), 1, &server)?;
    let two = Sidecar::open(scratch.0.join("two"), 2, &server)?;
    let payload = ApiPayload {
        method: METHOD_POST,
        level: LEVEL_SINGLE,
        attestor: one.address(),
        headers: vec![ApiHeader::new("Content-Type", "application/json")],
        body: b"{\"asset\":\"SID\",\"amount\":\"1\"}".to_vec(),
        pointers: vec!["/quote/amount".to_owned()],
        ..quote_payload(
            &server,
            vec![envelope(
                &one.key,
                7,
                &server.origin(),
                &[
                    ApiHeader::new(CREDENTIAL_HEADER, CREDENTIAL_ONE),
                    ApiHeader::new("X-Api-Secret", CREDENTIAL_TWO),
                ],
            )?],
        )
    };
    let request = request(9, &payload)?;

    let refused = two.attestor.attest(&request);
    assert_eq!(refused, Err(AttestError::NotNamed(one.address())));
    assert!(server.calls().is_empty());

    let answer = one.attestor.attest(&request)?;
    let calls = server.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "POST");
    assert_eq!(calls[0].body, payload.body);
    assert_eq!(calls[0].credential.as_deref(), Some(CREDENTIAL_ONE));
    assert!(calls[0]
        .headers
        .iter()
        .any(|(name, value)| name == "x-api-secret" && value == CREDENTIAL_TWO));
    assert!(calls[0]
        .headers
        .iter()
        .any(|(name, value)| name == "content-type" && value == "application/json"));
    assert_eq!(answer.level, Level::Single(one.address()));
    assert_eq!(answer.response, b"[\"3.114\"]".to_vec());
    assert_eq!(answer.attestation.full_length, 9);
    assert_eq!(
        recover_signer(&answer.digest, &answer.signature)?,
        one.address()
    );

    let set = AttestorSet {
        signers: vec![
            one.address(),
            two.address(),
            signer_address(&attestor_key(3)?),
        ],
        threshold: 2,
    };
    one.exchange.record(answer.clone());
    let mut second = answer.clone();
    second.signer = two.address();
    second.signature = sign_digest(&two.key, &answer.digest)?;
    assert_eq!(
        one.exchange.accept("two", 9, &second.record(), &set),
        Ok(Some(two.address()))
    );
    let ready = one
        .exchange
        .ready(9, &set)
        .ok_or_else(|| fail("the named attestor's signature is not ready"))?;
    assert_eq!(ready.signers, vec![one.address()]);
    assert_eq!(ready.signatures, vec![answer.signature]);
    let unregistered = AttestorSet {
        signers: vec![two.address()],
        threshold: 1,
    };
    assert_eq!(one.exchange.ready(9, &unregistered), None);

    let outcome = submit_one(&one.home, &ready, &answer.signature, &second.signature)?;

    let lines = vec![
        format!("{answer:?}"),
        answer.record().to_string(),
        format!("{ready:?}"),
        format!("{outcome:?}"),
        format!("{refused:?}"),
        format!("{:?}", one.exchange.discarded()),
    ];
    assert!(assert_credentials_absent(&scratch.0, &lines)? > 0);
    Ok(())
}
