use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[serde_with::serde_as]
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CborAuxdataValue {
    #[serde_as(as = "blockscout_display_bytes::serde_as::Hex")]
    pub value: Vec<u8>,
    pub offset: u32,
}
pub type CborAuxdata = BTreeMap<String, CborAuxdataValue>;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Offset {
    pub start: u32,
    pub length: u32,
}
pub type Offsets = Vec<Offset>;

pub type ImmutableReferences = BTreeMap<String, Offsets>;

pub type LinkReferences = BTreeMap<String, BTreeMap<String, Offsets>>;
