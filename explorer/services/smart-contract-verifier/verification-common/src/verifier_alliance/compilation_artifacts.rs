use serde::{Deserialize, Serialize};
use serde_json::Value;

pub trait ToCompilationArtifacts {
    fn abi(&self) -> Option<Value> {
        None
    }
    fn devdoc(&self) -> Option<Value> {
        None
    }
    fn userdoc(&self) -> Option<Value> {
        None
    }
    fn storage_layout(&self) -> Option<Value> {
        None
    }
}

impl<T: ToCompilationArtifacts> ToCompilationArtifacts for &T {
    fn abi(&self) -> Option<Value> {
        (*self).abi()
    }
    fn devdoc(&self) -> Option<Value> {
        (*self).devdoc()
    }
    fn userdoc(&self) -> Option<Value> {
        (*self).userdoc()
    }
    fn storage_layout(&self) -> Option<Value> {
        (*self).storage_layout()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceId {
    pub id: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompilationArtifacts {
    pub abi: Option<Value>,
    pub devdoc: Option<Value>,
    pub userdoc: Option<Value>,
    pub storage_layout: Option<Value>,
    pub sources: Option<Value>,
}

impl<T: ToCompilationArtifacts> From<T> for CompilationArtifacts {
    fn from(value: T) -> Self {
        Self {
            abi: value.abi(),
            devdoc: value.devdoc(),
            userdoc: value.userdoc(),
            storage_layout: value.storage_layout(),
            sources: None,
        }
    }
}

impl From<CompilationArtifacts> for Value {
    fn from(value: CompilationArtifacts) -> Self {
        serde_json::to_value(value).expect("compilation artifacts serialization must succeed")
    }
}
