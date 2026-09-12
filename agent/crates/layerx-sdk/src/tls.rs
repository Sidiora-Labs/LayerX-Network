use crate::programs::ProgramOperationError;

pub(crate) fn system_roots(endpoint: &str) -> Result<ureq::tls::RootCerts, ProgramOperationError> {
    let mut certificates = Vec::new();
    if endpoint.starts_with("https://") {
        let loaded = rustls_native_certs::load_native_certs();
        if loaded.certs.is_empty() || !loaded.errors.is_empty() {
            return Err(ProgramOperationError::Authentication);
        }
        certificates.extend(loaded.certs.iter().map(|certificate| {
            ureq::tls::Certificate::from_der(certificate.as_ref()).to_owned()
        }));
    }
    Ok(ureq::tls::RootCerts::new_with_certs(&certificates))
}
