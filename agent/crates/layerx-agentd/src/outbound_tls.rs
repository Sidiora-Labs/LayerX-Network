use ureq::tls::{Certificate, RootCerts, TlsConfig, TlsProvider};

pub(crate) fn private_ca(ca_der: &[u8]) -> Option<TlsConfig> {
    if ca_der.is_empty() {
        return None;
    }
    Some(
        TlsConfig::builder()
            .provider(TlsProvider::Rustls)
            .root_certs(RootCerts::new_with_certs(&[
                Certificate::from_der(ca_der).to_owned()
            ]))
            .build(),
    )
}

pub(crate) fn system(endpoint: &str) -> Option<TlsConfig> {
    let mut certificates = Vec::new();
    if endpoint
        .split_once("://")
        .is_some_and(|(scheme, _)| scheme.eq_ignore_ascii_case("https"))
    {
        let loaded = rustls_native_certs::load_native_certs();
        if loaded.certs.is_empty() || !loaded.errors.is_empty() {
            return None;
        }
        certificates.extend(
            loaded
                .certs
                .iter()
                .map(|certificate| Certificate::from_der(certificate.as_ref()).to_owned()),
        );
    }
    Some(
        TlsConfig::builder()
            .provider(TlsProvider::Rustls)
            .root_certs(RootCerts::new_with_certs(&certificates))
            .build(),
    )
}

#[cfg(test)]
#[path = "../../../../platform/tests/support/tls_boundary.rs"]
mod tls_boundary;

#[test]
fn private_ca_checks_the_actual_server_identity() {
    tls_boundary::qualify(
        "outbound_tls::private_ca_checks_the_actual_server_identity",
        |endpoint| {
            let ca = std::fs::read(
                std::env::var("LAYERX_TLS_QUAL_CA_DER").map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?;
            let tls = private_ca(&ca).ok_or("private trust root refused")?;
            let agent: ureq::Agent = ureq::Agent::config_builder()
                .tls_config(tls)
                .timeout_global(Some(std::time::Duration::from_secs(5)))
                .build()
                .into();
            agent
                .get(format!("{endpoint}/livez"))
                .call()
                .map_err(|error| error.to_string())?
                .body_mut()
                .read_to_vec()
                .map_err(|error| error.to_string())
        },
    );
}
