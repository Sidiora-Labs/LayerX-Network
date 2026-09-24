use crate::{
    test_success,
    types::{
        batch_solidity::{
            CompilationFailure, ContractVerificationFailure, ContractVerificationSuccess,
            MultiPart, StandardJson,
        },
        from_file,
    },
};

const TEST_CASES_DIR: &str = "tests/test_cases_batch_solidity";

#[tokio::test]
async fn basic_multi_part() {
    let (test_case_request, test_case_response) =
        from_file::<MultiPart, ContractVerificationSuccess>(TEST_CASES_DIR, "basic_multi_part");
    test_success(&test_case_request, &test_case_response).await;
}

#[tokio::test]
async fn basic_standard_json() {
    let (test_case_request, test_case_response) = from_file::<
        StandardJson,
        ContractVerificationSuccess,
    >(TEST_CASES_DIR, "basic_standard_json");
    test_success(&test_case_request, &test_case_response).await;
}

#[tokio::test]
async fn runtime_match_only_invalid_constructor_arguments() {
    let (test_case_request, test_case_response) =
        from_file::<StandardJson, ContractVerificationSuccess>(
            TEST_CASES_DIR,
            "runtime_match_only_invalid_constructor_arguments",
        );
    test_success(&test_case_request, &test_case_response).await;
}

#[tokio::test]
async fn compilation_error() {
    let (test_case_request, test_case_response) =
        from_file::<StandardJson, CompilationFailure>(TEST_CASES_DIR, "compilation_error");
    test_success(&test_case_request, &test_case_response).await;
}

#[tokio::test]
async fn invalid_standard_json() {
    let (test_case_request, test_case_response) =
        from_file::<StandardJson, CompilationFailure>(TEST_CASES_DIR, "invalid_standard_json");
    test_success(&test_case_request, &test_case_response).await;
}

#[tokio::test]
async fn failure_abstract_contract() {
    let (test_case_request, test_case_response) = from_file::<
        StandardJson,
        ContractVerificationFailure,
    >(TEST_CASES_DIR, "failure_abstract_contract");
    test_success(&test_case_request, &test_case_response).await;
}
