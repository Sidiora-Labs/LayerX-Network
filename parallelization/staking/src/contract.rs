use cosmwasm_std::{
    entry_point, StakingMsg, DepsMut, Env, MessageInfo,
    Response, StdError,
};
use crate::msg::{InstantiateMsg, ExecuteMsg};

#[entry_point]
pub fn instantiate(
    _: DepsMut,
    _env: Env,
    _: MessageInfo,
    _: InstantiateMsg,
) -> Result<Response, StdError> {
    Ok(Response::default())
}

#[entry_point]
pub fn execute(
    _: DepsMut,
    _: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, StdError> {
    match msg {
        ExecuteMsg::Delegate { validator } => delegate(info, validator),
    }
}

fn delegate(info: MessageInfo, validator: String) -> Result<Response, StdError> {
    if info.funds.len() != 1 {
        return Err(StdError::generic_err("delegate requires exactly one coin"));
    }
    let amount = info.funds[0].clone();
    if amount.amount.is_zero() {
        return Err(StdError::generic_err("delegate amount must be positive"));
    }
    let msg = StakingMsg::Delegate { validator, amount };
    Ok(Response::new().add_message(msg))
}

#[cfg(test)]
mod tests {
    use super::delegate;
    use cosmwasm_std::{coin, Addr, CosmosMsg, MessageInfo, StakingMsg};

    fn info(funds: Vec<cosmwasm_std::Coin>) -> MessageInfo {
        MessageInfo { sender: Addr::unchecked("delegator"), funds }
    }

    #[test]
    fn delegate_rejects_missing_multiple_and_zero_funds() {
        assert!(delegate(info(vec![]), "validator".to_string()).is_err());
        assert!(delegate(info(vec![coin(1, "uhpx"), coin(1, "other")]), "validator".to_string()).is_err());
        assert!(delegate(info(vec![coin(0, "uhpx")]), "validator".to_string()).is_err());
    }

    #[test]
    fn delegate_forwards_the_only_positive_coin() {
        let response = delegate(info(vec![coin(7, "uhpx")]), "validator".to_string()).unwrap();
        assert_eq!(response.messages.len(), 1);
        assert_eq!(
            response.messages[0].msg,
            CosmosMsg::Staking(StakingMsg::Delegate {
                validator: "validator".to_string(),
                amount: coin(7, "uhpx"),
            })
        );
    }
}
