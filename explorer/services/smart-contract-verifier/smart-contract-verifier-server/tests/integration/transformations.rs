use crate::types::{
    from_file,
    transformations::{TestCase, TestCaseMultiPart, TestCaseStandardJson},
    TestCaseRequest,
};

const TEST_CASES_DIR: &str = "tests/test_cases_solidity_transformations";

async fn test_success<Request: TestCaseRequest + for<'de> serde::Deserialize<'de>>(
    test_case: &str,
) {
    let (test_case_request, test_case_response) =
        from_file::<Request, TestCase>(TEST_CASES_DIR, test_case);
    crate::test_success(&test_case_request, &test_case_response).await;
}

async fn test_success_multi_part_and_standard_json(test_case: &str) {
    println!("Starting multi-part test case..");
    test_success::<TestCaseMultiPart>(test_case).await;
    println!("Multi-part test case succeeded, starting standard-json test case..");
    test_success::<TestCaseStandardJson>(test_case).await;
}

#[tokio::test]
async fn constructor_arguments() {
    test_success_multi_part_and_standard_json("constructor_arguments").await
}

#[tokio::test]
async fn full_match() {
    test_success_multi_part_and_standard_json("full_match").await
}

#[tokio::test]
async fn immutables() {
    test_success_multi_part_and_standard_json("immutables").await
}

#[tokio::test]
async fn libraries_linked_by_compiler() {
    // we do not support libraries specification in multi_part requests
    test_success::<TestCaseStandardJson>("libraries_linked_by_compiler").await;
}

#[tokio::test]
async fn libraries_manually_linked() {
    test_success_multi_part_and_standard_json("libraries_manually_linked").await
}

#[tokio::test]
async fn metadata_hash_absent() {
    // Now auxdata is not retrieved for contracts compiled without metadata hash.
    // TODO: should be removed, when that is fixed
    let remove_cbor_auxdata_from_artifacts = |artifacts: &mut serde_json::Value| {
        artifacts.as_object_mut().map(|artifacts| {
            if let Some(value) = artifacts.get_mut("cborAuxdata") {
                *value = serde_json::Value::Object(Default::default());
            }
        })
    };

    let (mut test_case_request, mut test_case_response) =
        from_file::<TestCaseStandardJson, TestCase>(TEST_CASES_DIR, "metadata_hash_absent");
    remove_cbor_auxdata_from_artifacts(&mut test_case_request.0.creation_code_artifacts);
    remove_cbor_auxdata_from_artifacts(&mut test_case_request.0.runtime_code_artifacts);
    remove_cbor_auxdata_from_artifacts(&mut test_case_response.creation_code_artifacts);
    remove_cbor_auxdata_from_artifacts(&mut test_case_response.runtime_code_artifacts);

    crate::test_success(&test_case_request, &test_case_response).await;
}

#[tokio::test]
async fn partial_match() {
    test_success_multi_part_and_standard_json("partial_match").await
}

#[tokio::test]
async fn partial_match_double_auxdata() {
    test_success_multi_part_and_standard_json("partial_match_double_auxdata").await
}
