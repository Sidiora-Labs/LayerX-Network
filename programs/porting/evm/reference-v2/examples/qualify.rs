use std::{env,fs};
use layerx_programs_runtime::{AbiError,ActivityBudgetBinding,AuthorizationContext,AuthorizedExecutionRequest,BudgetedAuthorizedExecutionRequest,CapabilitySet,CompositionContext,DeclaredBudget,ExecutionContext,Executor,PrincipalId,ProgramId,ReceiptOracle,ReceiptView,Storage,WasmEngine,CALL_ENTRY_EXPORT};
struct NoReceipts;
impl ReceiptOracle for NoReceipts { fn verified_receipt(&self,_:[u8;32])->Result<ReceiptView,AbiError>{Err(AbiError::ReceiptMismatch)} }
fn main(){
    for artifact in env::args().skip(1){
        let wasm=fs::read(&artifact).unwrap_or_else(|error|panic!("read {artifact}: {error}"));
        let module=WasmEngine::declared().unwrap_or_else(|error|panic!("engine: {error}")).validate_candidate_v2(&wasm).unwrap_or_else(|error|panic!("validate {artifact}: {error}"));
        let input=b"context-and-precompile";
        let executor=Executor::declared();let payer=PrincipalId::new([8;32]).unwrap_or_else(|error|panic!("principal: {error}"));let binding=ActivityBudgetBinding::new([9;32]).unwrap_or_else(|error|panic!("binding: {error}"));let token=executor.admit_activity_budget_for_qualification(DeclaredBudget::protocol_maximum(),payer,binding,u128::MAX).unwrap_or_else(|error|panic!("admit: {error}"));
        let request=BudgetedAuthorizedExecutionRequest::new(AuthorizedExecutionRequest{module:&module,program:ProgramId::new([6;32]).unwrap_or_else(|error|panic!("program: {error}")),authorization:AuthorizationContext::new(payer,CapabilitySet::empty()),receipts:&NoReceipts,entrypoint:CALL_ENTRY_EXPORT,calldata:input,composition:CompositionContext::isolated(),response_capacity:input.len()},token,payer,binding).with_execution_context_for_qualification(ExecutionContext::for_qualification(1,7,1,2,1).unwrap_or_else(|error|panic!("context: {error:?}")));
        let record=executor.execute_authorized_candidate_budgeted_for_qualification(&mut Storage::new(),request).unwrap_or_else(|error|panic!("execute {artifact}: {error}"));
        assert_eq!(record.response().unwrap_or_else(||panic!("response {artifact}")).bytes,input);
        assert_eq!(record.receipt_projection().abi_revision(),2);
    }
}
