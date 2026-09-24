use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum TransformationType {
    Insert,
    Replace,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum TransformationReason {
    CborAuxdata,
    ConstructorArguments,
    Immutable,
    Library,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Transformation {
    r#type: TransformationType,
    reason: TransformationReason,
    offset: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
}

impl From<Transformation> for serde_json::Value {
    fn from(value: Transformation) -> Self {
        serde_json::to_value(value).expect("transformations serialization must succeed")
    }
}

impl Transformation {
    pub fn auxdata(offset: usize, id: impl Into<String>) -> Self {
        Self {
            r#type: TransformationType::Replace,
            reason: TransformationReason::CborAuxdata,
            offset,
            id: Some(id.into()),
        }
    }

    pub fn constructor(offset: usize) -> Self {
        Self {
            r#type: TransformationType::Insert,
            reason: TransformationReason::ConstructorArguments,
            offset,
            id: None,
        }
    }

    pub fn immutable(offset: usize, id: impl Into<String>) -> Self {
        Self {
            r#type: TransformationType::Replace,
            reason: TransformationReason::Immutable,
            offset,
            id: Some(id.into()),
        }
    }

    pub fn library(offset: usize, id: impl Into<String>) -> Self {
        Self {
            r#type: TransformationType::Replace,
            reason: TransformationReason::Library,
            offset,
            id: Some(id.into()),
        }
    }
}
