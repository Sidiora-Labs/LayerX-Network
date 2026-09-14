use std::error::Error;

#[path = "native_budget_recovery/amend.rs"]
mod amend;
#[path = "native_budget_recovery/expiry.rs"]
mod expiry;
#[path = "native_budget_recovery/fees.rs"]
mod fees;
#[path = "native_budget_recovery/finality.rs"]
mod finality;
#[path = "native_budget_recovery/fixture.rs"]
mod fixture;
#[path = "native_budget_recovery/refusals.rs"]
mod refusals;
#[path = "native_budget_recovery/rotation.rs"]
mod rotation;
#[path = "native_budget_recovery/scenarios.rs"]
mod scenarios;

type Result<T> = std::result::Result<T, Box<dyn Error>>;
fn checked<T, E: std::fmt::Debug>(value: std::result::Result<T, E>) -> Result<T> {
    value.map_err(|error| format!("{error:?}").into())
}

fn main() -> Result<()> {
    let scenario = std::env::args()
        .nth(1)
        .ok_or("native Budget scenario is required")?;
    if std::env::args().len() != 2 {
        return Err("exactly one scenario is required".into());
    }
    let mut fixture = fixture::Fixture::open()?;
    match scenario.as_str() {
        "recovery" => scenarios::recovery(&mut fixture)?,
        "unknown" => scenarios::unknown(&mut fixture)?,
        "refusals" => refusals::run(&mut fixture)?,
        _ => return Err("unsupported native Budget scenario".into()),
    }
    println!("native Budget {scenario}: passed");
    Ok(())
}
