use lazy_static::lazy_static;
use prometheus::{
    register_gauge, register_histogram, register_int_counter, Gauge, Histogram, IntCounter,
};

lazy_static! {
    pub static ref DOWNLOAD_CACHE_TOTAL: IntCounter = register_int_counter!(
        "smart_contract_verifier_download_cache_total",
        "total number of get calls in DownloadCache",
    )
    .unwrap();
    pub static ref DOWNLOAD_CACHE_HITS: IntCounter = register_int_counter!(
        "smart_contract_verifier_download_cache_hits",
        "number of cache hits in DownloadCache",
    )
    .unwrap();
    pub static ref COMPILER_FETCH_TIME: Histogram = register_histogram!(
        "smart_contract_verifier_compiler_fetch_time_seconds",
        "download time for compilers in seconds",
        vec![0.01, 0.025, 0.05, 0.075, 0.1, 0.25, 0.5, 0.75, 1.0, 2.5, 5.0, 7.5, 10.0, 20.0],
    )
    .unwrap();
    pub static ref COMPILE_TIME: Histogram = register_histogram!(
        "smart_contract_verifier_compile_time_seconds",
        "contract compilation time in seconds",
    )
    .unwrap();
    pub static ref COMPILATIONS_IN_FLIGHT: Gauge = register_gauge!(
        "smart_contract_verifier_compiles_in_flight",
        "number of compilations currently running",
    )
    .unwrap();
    pub static ref COMPILATION_QUEUE_TIME: Histogram = register_histogram!(
        "smart_contract_verifier_compilation_queue_time_seconds",
        "waiting for the compilation queue in seconds",
    )
    .unwrap();
    pub static ref COMPILATIONS_IN_QUEUE: Gauge = register_gauge!(
        "smart_contract_verifier_compiles_in_queue",
        "number of compilations in queue",
    )
    .unwrap();
}

pub struct GaugeGuard(&'static Gauge);

impl Drop for GaugeGuard {
    fn drop(&mut self) {
        self.0.dec();
    }
}

pub trait GuardedGauge {
    fn guarded_inc(&'static self) -> GaugeGuard;
}

impl GuardedGauge for Gauge {
    fn guarded_inc(&'static self) -> GaugeGuard {
        self.inc();
        GaugeGuard(self)
    }
}
