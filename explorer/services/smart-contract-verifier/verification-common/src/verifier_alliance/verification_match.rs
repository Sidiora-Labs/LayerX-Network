use super::{
    code_artifact_types::{CborAuxdata, ImmutableReferences, LinkReferences},
    compilation_artifacts::CompilationArtifacts,
    creation_code_artifacts::CreationCodeArtifacts,
    runtime_code_artifacts::RuntimeCodeArtifacts,
};
pub use super::{
    verification_match_transformations::Transformation as MatchTransformation,
    verification_match_values::Values as MatchValues,
};
use alloy_dyn_abi::JsonAbiExt;
use anyhow::{anyhow, Context};
use bytes::Bytes;
use serde::Deserialize;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    pub metadata_match: bool,
    pub transformations: Vec<MatchTransformation>,
    pub values: MatchValues,
}

pub fn verify_creation_code(
    on_chain_code: &[u8],
    compiled_code: Vec<u8>,
    creation_code_artifacts: &CreationCodeArtifacts,
    compilation_artifacts: &CompilationArtifacts,
) -> Result<Option<Match>, anyhow::Error> {
    let builder = MatchBuilder::new(on_chain_code, compiled_code);
    if let Some(builder) = builder {
        return Ok(builder
            .apply_creation_code_transformations(creation_code_artifacts, compilation_artifacts)?
            .verify_and_build());
    }
    Ok(None)
}

pub fn verify_runtime_code(
    on_chain_code: &[u8],
    compiled_code: Vec<u8>,
    runtime_code_artifacts: &RuntimeCodeArtifacts,
) -> Result<Option<Match>, anyhow::Error> {
    let builder = MatchBuilder::new(on_chain_code, compiled_code);
    if let Some(builder) = builder {
        return Ok(builder
            .apply_runtime_code_transformations(runtime_code_artifacts)?
            .verify_and_build());
    }
    Ok(None)
}

pub fn verify_blueprint_initcode(
    initcode: &[u8],
    compiled_code: Vec<u8>,
    creation_code_artifacts: &CreationCodeArtifacts,
) -> Result<Option<Match>, anyhow::Error> {
    let builder = MatchBuilder::new(initcode, compiled_code);
    if let Some(builder) = builder {
        return Ok(builder
            .apply_blueprint_initcode_transformations(creation_code_artifacts)?
            .verify_and_build());
    }
    Ok(None)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchBuilder<'a> {
    deployed_code: &'a [u8],
    compiled_code: Vec<u8>,
    transformations: Vec<MatchTransformation>,
    values: MatchValues,
    invalid_constructor_arguments: bool,
    has_cbor_auxdata: bool,
    has_cbor_auxdata_transformation: bool,
}

impl<'a> MatchBuilder<'a> {
    pub fn new(deployed_code: &'a [u8], compiled_code: Vec<u8>) -> Option<Self> {
        if deployed_code.len() < compiled_code.len() {
            return None;
        }

        Some(Self {
            deployed_code,
            compiled_code,
            transformations: vec![],
            values: MatchValues::default(),
            invalid_constructor_arguments: false,
            has_cbor_auxdata: false,
            has_cbor_auxdata_transformation: false,
        })
    }

    pub fn set_has_cbor_auxdata(mut self, value: bool) -> Self {
        self.has_cbor_auxdata = value;
        self
    }

    pub fn apply_runtime_code_transformations(
        self,
        runtime_code_artifacts: &RuntimeCodeArtifacts,
    ) -> Result<Self, anyhow::Error> {
        self.apply_cbor_auxdata_transformations(runtime_code_artifacts.cbor_auxdata.as_ref())?
            .apply_library_transformations(runtime_code_artifacts.link_references.as_ref())?
            .apply_immutable_transformations(runtime_code_artifacts.immutable_references.as_ref())
    }

    pub fn apply_creation_code_transformations(
        self,
        creation_code_artifacts: &CreationCodeArtifacts,
        compilation_artifacts: &CompilationArtifacts,
    ) -> Result<Self, anyhow::Error> {
        self.apply_cbor_auxdata_transformations(creation_code_artifacts.cbor_auxdata.as_ref())?
            .apply_library_transformations(creation_code_artifacts.link_references.as_ref())?
            .apply_constructor_transformation(compilation_artifacts.abi.as_ref())
    }

    pub fn apply_blueprint_initcode_transformations(
        self,
        creation_code_artifacts: &CreationCodeArtifacts,
    ) -> Result<Self, anyhow::Error> {
        self.apply_cbor_auxdata_transformations(creation_code_artifacts.cbor_auxdata.as_ref())
    }

    pub fn verify_and_build(self) -> Option<Match> {
        if !self.invalid_constructor_arguments
            && self.deployed_code == self.compiled_code.as_slice()
        {
            let metadata_match = self.has_cbor_auxdata && !self.has_cbor_auxdata_transformation;
            return Some(Match {
                metadata_match,
                transformations: self.transformations,
                values: self.values,
            });
        }

        None
    }

    fn apply_cbor_auxdata_transformations(
        mut self,
        cbor_auxdata: Option<&CborAuxdata>,
    ) -> Result<Self, anyhow::Error> {
        let cbor_auxdata = match cbor_auxdata {
            Some(cbor_auxdata) => cbor_auxdata,
            None => return Ok(self),
        };

        self.has_cbor_auxdata = !cbor_auxdata.is_empty();
        for (id, cbor_auxdata_value) in cbor_auxdata {
            let offset = cbor_auxdata_value.offset as usize;
            let re_compiled_value = cbor_auxdata_value.value.to_vec();

            let range = offset..offset + re_compiled_value.len();

            if self.compiled_code.len() < range.end {
                return Err(anyhow!("(reason=cbor_auxdata; id={id}) out of range"));
            }

            let on_chain_value = &self.deployed_code[range.clone()];
            if on_chain_value != re_compiled_value {
                self.has_cbor_auxdata_transformation = true;
                self.compiled_code.as_mut_slice()[range].copy_from_slice(on_chain_value);

                self.transformations
                    .push(MatchTransformation::auxdata(offset, id));
                self.values.add_cbor_auxdata(id, on_chain_value.to_vec());
            }
        }

        Ok(self)
    }

    fn apply_library_transformations(
        mut self,
        link_references: Option<&LinkReferences>,
    ) -> Result<Self, anyhow::Error> {
        let link_references = match link_references {
            Some(link_references) => link_references,
            None => return Ok(self),
        };

        for (file, file_references) in link_references {
            for (contract, offsets) in file_references {
                let id = format!("{file}:{contract}");
                let mut on_chain_value = None;
                for offset in offsets {
                    let start = offset.start as usize;
                    let end = start + offset.length as usize;
                    let range = start..end;

                    let offset_value = &self.deployed_code[range.clone()];
                    match on_chain_value {
                        None => {
                            on_chain_value = Some(offset_value);
                        }
                        Some(on_chain_value) if on_chain_value != offset_value => {
                            return Err(anyhow!(
                                "(reason=link_reference; id={id}) offset values are not consistent"
                            ))
                        }
                        _ => {}
                    }

                    self.compiled_code.as_mut_slice()[range].copy_from_slice(offset_value);
                    self.transformations
                        .push(MatchTransformation::library(start, &id));
                    self.values.add_library(&id, offset_value.to_vec());
                }
            }
        }

        Ok(self)
    }

    fn apply_immutable_transformations(
        mut self,
        immutable_references: Option<&ImmutableReferences>,
    ) -> Result<Self, anyhow::Error> {
        let immutable_references = match immutable_references {
            Some(immutable_references) => immutable_references,
            None => return Ok(self),
        };

        for (id, offsets) in immutable_references {
            let mut on_chain_value = None;
            for offset in offsets {
                let start = offset.start as usize;
                let end = start + offset.length as usize;
                let range = start..end;

                let offset_value = &self.deployed_code[range.clone()];
                match on_chain_value {
                    None => {
                        on_chain_value = Some(offset_value);
                    }
                    Some(on_chain_value) if on_chain_value != offset_value => {
                        return Err(anyhow!(
                            "(reason=immutable_reference; id={id}) offset values are not consistent"
                        ))
                    }
                    _ => {}
                }

                self.compiled_code.as_mut_slice()[range].copy_from_slice(offset_value);
                self.transformations
                    .push(MatchTransformation::immutable(start, id));
                self.values.add_immutable(id, offset_value.to_vec());
            }
        }

        Ok(self)
    }

    fn apply_constructor_transformation(
        mut self,
        abi: Option<&serde_json::Value>,
    ) -> Result<Self, anyhow::Error> {
        let offset = self.compiled_code.len();
        let (_prefix, constructor_arguments) = self.deployed_code.split_at(offset);

        let constructor = match abi {
            Some(abi) => {
                alloy_json_abi::JsonAbi::deserialize(abi)
                    .context("parsing compiled contract abi")?
                    .constructor
            }
            None => None,
        };

        match constructor {
            None if !constructor_arguments.is_empty() => {
                self.invalid_constructor_arguments = true;
            }
            None => {}
            Some(constructor)
                if try_decode_constructor_arguments(&constructor, constructor_arguments)
                    .is_err() =>
            {
                self.invalid_constructor_arguments = true;
            }
            Some(_constructor) if constructor_arguments.is_empty() => {}
            Some(_constructor) => {
                self.compiled_code.extend(constructor_arguments);
                self.transformations
                    .push(MatchTransformation::constructor(offset));
                self.values
                    .add_constructor_arguments(Bytes::copy_from_slice(constructor_arguments));
            }
        }

        Ok(self)
    }
}

/// A simple `alloy_json_abi::abi_decode_input` does not verify that the whole data are used during decoding.
/// This function makes sure that no data are left after.
///
/// If not validate that then abstract contracts
/// That is required, as all remaining bytes must correspond to constructor arguments in order for verification to succeed.
fn try_decode_constructor_arguments(
    constructor: &alloy_json_abi::Constructor,
    data: &[u8],
) -> Result<Vec<alloy_dyn_abi::DynSolValue>, alloy_dyn_abi::Error> {
    let decoded_input = constructor.abi_decode_input(data, true)?;
    let encoded_data = constructor.abi_encode_input(&decoded_input)?;
    if data != encoded_data {
        return Err(alloy_dyn_abi::Error::custom(
            "not all data has been used for decoding",
        ));
    }
    Ok(decoded_input)
}
