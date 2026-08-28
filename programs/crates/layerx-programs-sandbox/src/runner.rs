use layerx_programs_runtime::{FeeSchedule, Storage};

use crate::execute::{
    execute_scoped, SandboxExecutionRecord, SandboxExecutionRequest, SandboxRefusal,
};
use crate::Lease;

pub fn execute(
    storage: &mut Storage,
    lease: &Lease,
    prices: FeeSchedule,
    request: SandboxExecutionRequest<'_>,
) -> Result<SandboxExecutionRecord, SandboxRefusal> {
    execute_scoped(storage, lease, prices, request)
}
