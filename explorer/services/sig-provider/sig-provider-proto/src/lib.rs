#![allow(clippy::derive_partial_eq_without_eq)]
pub mod blockscout {
    pub mod sig_provider {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/blockscout.sig_provider.v1.rs"));
        }
    }
}
