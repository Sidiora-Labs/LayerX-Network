use lazy_static::lazy_static;
use prometheus::{register_int_counter_vec, IntCounterVec};

lazy_static! {
    pub static ref VERIFICATION: IntCounterVec = register_int_counter_vec!(
        "smart_contract_verifier_verify_contract",
        "number of contract verifications",
        &["chain_id", "language", "endpoint", "status"],
    )
    .unwrap();
}

pub fn count_verify_contract(chain_id: &str, language: &str, status: &str, method: &str) {
    VERIFICATION
        .with_label_values(&[chain_id, language, method, status])
        .inc();
}
