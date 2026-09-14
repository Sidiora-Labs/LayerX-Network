use std::error::Error;

#[path = "native_managed_limit/creation.rs"]
mod creation;
#[path = "../../../../agent/crates/layerx-agentd/examples/native_budget_recovery/fees.rs"]
pub mod fees;
#[path = "../../../../agent/crates/layerx-agentd/examples/native_budget_recovery/finality.rs"]
pub mod finality;
#[path = "../../../../agent/crates/layerx-agentd/examples/native_budget_recovery/fixture.rs"]
pub mod fixture;
#[path = "native_managed_limit/identity.rs"]
mod identity;
#[path = "native_managed_limit/limit.rs"]
mod limit;
#[path = "native_managed_limit/onboarding.rs"]
mod onboarding;
#[path = "native_managed_limit/owner.rs"]
mod owner;
#[path = "native_managed_limit/transport.rs"]
mod transport;

type Result<T> = std::result::Result<T, Box<dyn Error>>;
#[track_caller]
fn checked<T, E: std::fmt::Debug>(value: std::result::Result<T, E>) -> Result<T> {
    let location = std::panic::Location::caller();
    value.map_err(|error| format!("{location}: {error:?}").into())
}

fn main() -> Result<()> {
    if std::env::args()
        .collect::<Vec<_>>()
        .get(1)
        .map(String::as_str)
        != Some("managed")
        || std::env::args().len() != 2
    {
        return Err("exact managed scenario required".into());
    }
    let mut native = fixture::Fixture::open()?;
    let created = creation::create(&mut native)?;
    let installed = owner::install(&mut native, created)?;
    limit::qualify(&mut native, installed)?;
    println!("native managed Budget AMEND: passed");
    Ok(())
}
