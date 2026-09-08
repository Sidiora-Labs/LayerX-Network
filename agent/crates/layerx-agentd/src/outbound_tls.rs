use ureq::tls::{Certificate, RootCerts, TlsConfig, TlsProvider};

pub(crate) fn private_ca(ca_der: &[u8]) -> Option<TlsConfig> {
    if ca_der.is_empty() {
        return None;
    }
    Some(
        TlsConfig::builder()
            .provider(TlsProvider::NativeTls)
            .root_certs(RootCerts::new_with_certs(&[
                Certificate::from_der(ca_der).to_owned()
            ]))
            .build(),
    )
}
