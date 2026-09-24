use actix_prost_build::{ActixGenerator, GeneratorList};
use prost_build::{Config, ServiceGenerator};
use std::path::Path;

// custom function to include custom generator
fn compile(
    protos: &[impl AsRef<Path>],
    includes: &[impl AsRef<Path>],
    generator: Box<dyn ServiceGenerator>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = Config::new();
    config
        .service_generator(generator)
        .compile_well_known_types()
        .protoc_arg("--openapiv2_out=swagger/v2")
        .protoc_arg("--openapiv2_opt")
        .protoc_arg("grpc_api_configuration=proto/v2/api_config_http.yaml,output_format=yaml,allow_merge=true,merge_file_name=smart-contract-verifier")
        .bytes(["."])
        .btree_map(["."])
        .type_attribute(".", "#[actix_prost_macros::serde]")
        .field_attribute(
            ".blockscout.smartContractVerifier.v2.HealthCheckRequest.service",
            "#[serde(default)]"
        )
        .field_attribute(
            ".blockscout.smartContractVerifier.v2.VerifyVyperMultiPartRequest.interfaces",
            "#[serde(default)]"
        )
        .field_attribute(
            ".blockscout.smartContractVerifier.v2.VerifySolidityMultiPartRequest.post_actions",
            "#[serde(default)]"
        )
        .field_attribute(
            ".blockscout.smartContractVerifier.v2.VerifySolidityStandardJsonRequest.post_actions",
            "#[serde(default)]"
        );
    config.compile_protos(protos, includes)?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // We need to rebuild proto lib only if any of proto definitions
    // (or corresponding http mapping) has been changed.
    println!("cargo:rerun-if-changed=proto/");

    std::fs::create_dir_all("./swagger/v2").unwrap();
    let gens = Box::new(GeneratorList::new(vec![
        tonic_build::configure().service_generator(),
        Box::new(ActixGenerator::new("proto/v2/api_config_http.yaml").unwrap()),
    ]));
    compile(
        &[
            "proto/v2/smart-contract-verifier.proto",
            "proto/v2/zksync-solidity.proto",
            "proto/v2/health.proto",
        ],
        &["proto"],
        gens,
    )?;
    Ok(())
}
