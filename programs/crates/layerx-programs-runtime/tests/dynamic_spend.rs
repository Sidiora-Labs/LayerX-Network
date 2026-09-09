use layerx_programs_runtime::{
    derive_program_account, dynamic_spend::CallerAuthorizedSpend, Capability, ProgramId,
};

#[test]
fn dynamic_spend_requires_exact_caller_authorization_on_each_call() {
    let program = ProgramId::new([1; 32]).unwrap_or_else(|e| panic!("{e}"));
    let source = derive_program_account(program, b"token").unwrap_or_else(|e| panic!("{e}"));
    let descriptor = CallerAuthorizedSpend {
        asset: [2; 32],
        maximum_amount: 100,
        recipient_offset: 10,
        amount_offset: 42,
    };
    let grant = Capability::ProgramSpend {
        owner_program: program,
        seed: b"token".to_vec(),
        source_account: source.bytes(),
        asset: [2; 32],
        to: [3; 32],
        maximum_amount: 50,
    };
    let mut input = vec![0; 58];
    input[10..42].copy_from_slice(&[3; 32]);
    input[42..].copy_from_slice(&50_u128.to_be_bytes());
    assert!(descriptor
        .authorize(program, &input, std::slice::from_ref(&grant))
        .is_ok());
    assert!(descriptor.authorize(program, &input, &[]).is_err());
    for length in 0..input.len() {
        assert!(descriptor
            .authorize(program, &input[..length], std::slice::from_ref(&grant))
            .is_err());
    }
    for amount in [0_u128, 51, 101, u128::MAX] {
        input[42..].copy_from_slice(&amount.to_be_bytes());
        assert!(descriptor
            .authorize(program, &input, std::slice::from_ref(&grant))
            .is_err());
    }
    input[42..].copy_from_slice(&50_u128.to_be_bytes());
    for field in 0..5 {
        let mut changed = grant.clone();
        if let Capability::ProgramSpend {
            owner_program,
            seed,
            source_account,
            asset,
            to,
            ..
        } = &mut changed
        {
            match field {
                0 => *owner_program = ProgramId::new([9; 32]).unwrap_or_else(|e| panic!("{e}")),
                1 => seed.push(0),
                2 => source_account[0] ^= 1,
                3 => asset[0] ^= 1,
                _ => to[0] ^= 1,
            }
        }
        assert!(descriptor.authorize(program, &input, &[changed]).is_err());
    }
    input[10] ^= 1;
    assert!(descriptor.authorize(program, &input, &[grant]).is_err());
}

#[test]
fn dynamic_policy_narrows_broad_grants_before_the_guest_can_spend() {
    let program = ProgramId::new([1; 32]).unwrap_or_else(|e| panic!("{e}"));
    let source = derive_program_account(program, b"token").unwrap_or_else(|e| panic!("{e}"));
    let descriptor = CallerAuthorizedSpend {
        asset: [2; 32],
        maximum_amount: 100,
        recipient_offset: 10,
        amount_offset: 42,
    };
    let mut input = vec![0; 58];
    input[10..42].copy_from_slice(&[3; 32]);
    input[42..].copy_from_slice(&50_u128.to_be_bytes());
    let broad = Capability::ProgramSpend {
        owner_program: program,
        seed: b"token".to_vec(),
        source_account: source.bytes(),
        asset: [2; 32],
        to: [3; 32],
        maximum_amount: 500,
    };
    let mut other_recipient = broad.clone();
    if let Capability::ProgramSpend { to, .. } = &mut other_recipient {
        *to = [4; 32];
    }
    let mut other_asset = broad.clone();
    if let Capability::ProgramSpend { asset, .. } = &mut other_asset {
        *asset = [5; 32];
    }
    let grants = vec![
        broad.clone(),
        other_recipient,
        other_asset,
        Capability::StorageRead,
    ];
    let before = grants.clone();
    let narrowed = CallerAuthorizedSpend::constrain_grants(program, &input, &[descriptor], &grants)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(grants, before);
    let mut expected = broad;
    if let Capability::ProgramSpend { maximum_amount, .. } = &mut expected {
        *maximum_amount = 100;
    }
    assert_eq!(narrowed, vec![expected, Capability::StorageRead]);
    assert!(layerx_programs_runtime::CapabilitySet::new(narrowed).is_ok());
    assert_eq!(
        CallerAuthorizedSpend::constrain_grants(program, &input, &[], &grants),
        Ok(vec![Capability::StorageRead])
    );
}
