use bytes::Bytes;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use std::collections::BTreeMap;

#[serde_as]
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[readonly::make]
pub struct Values {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[serde_as(as = "BTreeMap<_, blockscout_display_bytes::serde_as::Hex>")]
    pub cbor_auxdata: BTreeMap<String, Bytes>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde_as(as = "Option<blockscout_display_bytes::serde_as::Hex>")]
    pub constructor_arguments: Option<Bytes>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[serde_as(as = "BTreeMap<_, blockscout_display_bytes::serde_as::Hex>")]
    pub libraries: BTreeMap<String, Bytes>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[serde_as(as = "BTreeMap<_, blockscout_display_bytes::serde_as::Hex>")]
    pub immutables: BTreeMap<String, Bytes>,
}

impl From<Values> for serde_json::Value {
    fn from(value: Values) -> Self {
        serde_json::to_value(value).expect("values serialization must succeed")
    }
}

impl Values {
    pub fn add_cbor_auxdata(
        &mut self,
        key: impl Into<String>,
        value: impl Into<Bytes>,
    ) -> &mut Self {
        self.cbor_auxdata.insert(key.into(), value.into());
        self
    }

    pub fn add_constructor_arguments(&mut self, value: impl Into<Bytes>) -> &mut Self {
        self.constructor_arguments = Some(value.into());
        self
    }

    pub fn add_library(&mut self, key: impl Into<String>, value: impl Into<Bytes>) -> &mut Self {
        self.libraries.insert(key.into(), value.into());
        self
    }

    pub fn add_immutable(&mut self, key: impl Into<String>, value: impl Into<Bytes>) -> &mut Self {
        self.immutables.insert(key.into(), value.into());
        self
    }
}
