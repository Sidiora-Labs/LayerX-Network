use std::error::Error;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

use native_tls::{Identity, TlsAcceptor};
use openssl::asn1::Asn1Time;
use openssl::bn::{BigNum, MsbOption};
use openssl::hash::MessageDigest;
use openssl::pkey::{PKey, Private};
use openssl::rsa::Rsa;
use openssl::x509::extension::{
    BasicConstraints, ExtendedKeyUsage, KeyUsage, SubjectAlternativeName,
};
use openssl::x509::{X509NameBuilder, X509};

use crate::human::HumanOperationError;
use crate::human_runtime::RemoteHumanAuthority;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

fn certificate(issuer: Option<(&X509, &PKey<Private>)>) -> TestResult<(X509, PKey<Private>)> {
    let key = PKey::from_rsa(Rsa::generate(2048)?)?;
    let mut name = X509NameBuilder::new()?;
    name.append_entry_by_text(
        "CN",
        if issuer.is_some() {
            "localhost"
        } else {
            "LayerX test CA"
        },
    )?;
    let name = name.build();
    let mut cert = X509::builder()?;
    cert.set_version(2)?;
    let mut serial = BigNum::new()?;
    serial.rand(128, MsbOption::MAYBE_ZERO, false)?;
    cert.set_serial_number(serial.to_asn1_integer()?.as_ref())?;
    cert.set_subject_name(&name)?;
    cert.set_issuer_name(issuer.map_or(&name, |(ca, _)| ca.subject_name()))?;
    cert.set_pubkey(&key)?;
    cert.set_not_before(Asn1Time::days_from_now(0)?.as_ref())?;
    cert.set_not_after(Asn1Time::days_from_now(1)?.as_ref())?;
    if let Some((ca, signing_key)) = issuer {
        cert.append_extension(BasicConstraints::new().critical().build()?)?;
        cert.append_extension(
            KeyUsage::new()
                .digital_signature()
                .key_encipherment()
                .build()?,
        )?;
        cert.append_extension(ExtendedKeyUsage::new().server_auth().build()?)?;
        let san = SubjectAlternativeName::new()
            .dns("localhost")
            .build(&cert.x509v3_context(Some(ca), None))?;
        cert.append_extension(san)?;
        cert.sign(signing_key, MessageDigest::sha256())?;
    } else {
        cert.append_extension(BasicConstraints::new().critical().ca().build()?)?;
        cert.append_extension(
            KeyUsage::new()
                .critical()
                .key_cert_sign()
                .crl_sign()
                .build()?,
        )?;
        cert.sign(&key, MessageDigest::sha256())?;
    }
    Ok((cert.build(), key))
}

fn exchange(identity: &Identity, ca: &[u8], host: &str) -> TestResult<(bool, bool)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let acceptor = TlsAcceptor::new(identity.clone())?;
    let server = thread::spawn(move || -> Result<bool, String> {
        let (stream, _) = listener.accept().map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| e.to_string())?;
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| e.to_string())?;
        let Ok(mut tls) = acceptor.accept(stream) else {
            return Ok(false);
        };
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") && request.len() < 4096 {
            let mut byte = [0];
            if tls.read(&mut byte).map_err(|e| e.to_string())? == 0 {
                return Ok(false);
            }
            request.push(byte[0]);
        }
        if !request.starts_with(b"GET / HTTP/1.1\r\n") {
            return Err("unexpected TLS transport request".to_owned());
        }
        tls.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ntrue")
            .map_err(|e| e.to_string())?;
        Ok(true)
    });
    let authority = RemoteHumanAuthority::connect(
        &format!("https://{host}:{port}"),
        "a".repeat(32),
        Duration::from_secs(5),
        1024,
        ca,
    )
    .map_err(|_| "invalid authority configuration")?;
    let accepted = authority
        .get("/")
        .is_ok_and(|value| value == serde_json::Value::Bool(true));
    let handled = server.join().map_err(|_| "TLS server panicked")??;
    Ok((accepted, handled))
}

#[test]
fn private_ca_is_required_and_hostname_verification_remains_enabled() -> TestResult {
    let (ca, ca_key) = certificate(None)?;
    let (other_ca, _) = certificate(None)?;
    let (server_cert, server_key) = certificate(Some((&ca, &ca_key)))?;
    let identity = Identity::from_pkcs8(
        &server_cert.to_pem()?,
        &server_key.private_key_to_pem_pkcs8()?,
    )?;
    assert_eq!(
        exchange(&identity, &ca.to_der()?, "localhost")?,
        (true, true)
    );
    assert_eq!(
        exchange(&identity, &other_ca.to_der()?, "localhost")?,
        (false, false)
    );
    assert_eq!(
        exchange(&identity, &ca.to_der()?, "127.0.0.1")?,
        (false, false)
    );
    Ok(())
}

#[test]
fn human_authority_refuses_empty_ca_and_preserves_input_bounds() -> TestResult {
    assert!(crate::outbound_tls::private_ca(&[]).is_none());
    let result = RemoteHumanAuthority::connect(
        "https://localhost",
        "a".repeat(32),
        Duration::from_secs(1),
        1024,
        &[],
    );
    assert!(matches!(result, Err(HumanOperationError::Refused)));
    let ca = certificate(None)?.0.to_der()?;
    for (endpoint, bearer, deadline, bound) in [
        (
            "http://localhost",
            "a".repeat(32),
            Duration::from_secs(1),
            1024,
        ),
        (
            "https://localhost",
            "a".repeat(31),
            Duration::from_secs(1),
            1024,
        ),
        ("https://localhost", "a".repeat(32), Duration::ZERO, 1024),
        (
            "https://localhost",
            "a".repeat(32),
            Duration::from_secs(1),
            0,
        ),
        (
            "https://localhost",
            "a".repeat(32),
            Duration::from_secs(1),
            usize::MAX,
        ),
    ] {
        assert!(matches!(
            RemoteHumanAuthority::connect(endpoint, bearer, deadline, bound, &ca),
            Err(HumanOperationError::Refused)
        ));
    }
    Ok(())
}
