use layerx_programs_interpreter::Interpreter;

const SUCCESSES: &[u8] = include_bytes!("../vectors/v1-arithmetic.hex");
const REFUSALS: &[u8] = include_bytes!("../vectors/v1-refusals.hex");

#[derive(Debug, Default, Eq, PartialEq)]
struct OracleState {
    storage: Vec<(Vec<u8>, i64)>,
    transfers: Vec<([u8; 32], [u8; 32], i64)>,
}

fn nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn vectors(source: &[u8]) -> Vec<Vec<u8>> {
    std::str::from_utf8(source)
        .unwrap_or_else(|error| panic!("vector utf8: {error}"))
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            assert_eq!(line.len() % 2, 0);
            line.as_bytes()
                .chunks_exact(2)
                .map(|pair| {
                    let high = nibble(pair[0]).unwrap_or_else(|| panic!("high hex"));
                    let low = nibble(pair[1]).unwrap_or_else(|| panic!("low hex"));
                    (high << 4) | low
                })
                .collect()
        })
        .collect()
}

fn take<'a>(bytes: &'a [u8], offset: &mut usize, length: usize) -> Result<&'a [u8], ()> {
    let end = offset.checked_add(length).ok_or(())?;
    let value = bytes.get(*offset..end).ok_or(())?;
    *offset = end;
    Ok(value)
}

fn register(bytes: &[u8], offset: &mut usize, count: usize) -> Result<usize, ()> {
    let value = usize::from(take(bytes, offset, 1)?[0]);
    if value < count { Ok(value) } else { Err(()) }
}

fn inspect(code: &[u8], registers: usize, depth: u8, remaining: &mut u32) -> Result<(), ()> {
    if depth > 4 { return Err(()); }
    let mut offset = 0;
    while offset < code.len() {
        *remaining = remaining.checked_sub(1).ok_or(())?;
        match take(code, &mut offset, 1)?[0] {
            0x00 => {}
            0x01 => { register(code, &mut offset, registers)?; take(code, &mut offset, 8)?; }
            0x02..=0x07 => {
                register(code, &mut offset, registers)?;
                register(code, &mut offset, registers)?;
                register(code, &mut offset, registers)?;
            }
            0x08 | 0x09 => {
                register(code, &mut offset, registers)?;
                let length = usize::from(take(code, &mut offset, 1)?[0]);
                if length == 0 { return Err(()); }
                take(code, &mut offset, length)?;
            }
            0x0a => {
                let length = usize::from(take(code, &mut offset, 1)?[0]);
                if length == 0 { return Err(()); }
                take(code, &mut offset, length)?;
            }
            0x0b => {
                register(code, &mut offset, registers)?;
                let asset = take(code, &mut offset, 32)?;
                let recipient = take(code, &mut offset, 32)?;
                if asset.iter().all(|byte| *byte == 0) || recipient.iter().all(|byte| *byte == 0) { return Err(()); }
            }
            0x0c => {
                let count = u16::from_be_bytes(take(code, &mut offset, 2)?.try_into().map_err(|_| ())?);
                let length = usize::from(u16::from_be_bytes(take(code, &mut offset, 2)?.try_into().map_err(|_| ())?));
                if count == 0 || length == 0 { return Err(()); }
                let body = take(code, &mut offset, length)?;
                let mut body_remaining = 4_096;
                inspect(body, registers, depth.checked_add(1).ok_or(())?, &mut body_remaining)?;
                let body_steps = 4_096_u32.checked_sub(body_remaining).ok_or(())?;
                *remaining = remaining.checked_sub(body_steps.checked_mul(u32::from(count)).ok_or(())?).ok_or(())?;
            }
            _ => return Err(()),
        }
    }
    Ok(())
}

fn execute_block(
    code: &[u8],
    register_count: usize,
    registers: &mut [i64; 16],
    state: &mut OracleState,
    steps: &mut u32,
    ceiling: u32,
) -> Result<bool, ()> {
    let mut offset = 0;
    while offset < code.len() {
        *steps = steps.checked_add(1).ok_or(())?;
        if *steps > ceiling { return Err(()); }
        match take(code, &mut offset, 1)?[0] {
            0x00 => return Ok(true),
            0x01 => {
                let destination = register(code, &mut offset, register_count)?;
                registers[destination] = i64::from_be_bytes(take(code, &mut offset, 8)?.try_into().map_err(|_| ())?);
            }
            opcode @ 0x02..=0x07 => {
                let destination = register(code, &mut offset, register_count)?;
                let left = register(code, &mut offset, register_count)?;
                let right = register(code, &mut offset, register_count)?;
                registers[destination] = match opcode {
                    0x02 => registers[left].checked_add(registers[right]).ok_or(())?,
                    0x03 => registers[left].checked_sub(registers[right]).ok_or(())?,
                    0x04 => registers[left].checked_mul(registers[right]).ok_or(())?,
                    0x05 => registers[left].checked_div(registers[right]).ok_or(())?,
                    0x06 => i64::from(registers[left] == registers[right]),
                    0x07 => i64::from(registers[left] < registers[right]),
                    _ => return Err(()),
                };
            }
            opcode @ (0x08 | 0x09) => {
                let selected = register(code, &mut offset, register_count)?;
                let length = usize::from(take(code, &mut offset, 1)?[0]);
                let key = take(code, &mut offset, length)?;
                if opcode == 0x08 {
                    registers[selected] = state.storage.iter()
                        .find(|(candidate, _)| candidate == key).map_or(0, |(_, value)| *value);
                } else if let Some((_, value)) = state.storage.iter_mut().find(|(candidate, _)| candidate == key) {
                    *value = registers[selected];
                } else {
                    state.storage.push((key.to_vec(), registers[selected]));
                }
            }
            0x0a => {
                let length = usize::from(take(code, &mut offset, 1)?[0]);
                let key = take(code, &mut offset, length)?;
                state.storage.retain(|(candidate, _)| candidate != key);
            }
            0x0b => {
                let amount = register(code, &mut offset, register_count)?;
                let asset = take(code, &mut offset, 32)?.try_into().map_err(|_| ())?;
                let recipient = take(code, &mut offset, 32)?.try_into().map_err(|_| ())?;
                if registers[amount] <= 0 { return Err(()); }
                state.transfers.push((asset, recipient, registers[amount]));
            }
            0x0c => {
                let count = u16::from_be_bytes(take(code, &mut offset, 2)?.try_into().map_err(|_| ())?);
                let length = usize::from(u16::from_be_bytes(take(code, &mut offset, 2)?.try_into().map_err(|_| ())?));
                let body = take(code, &mut offset, length)?;
                for _ in 0..count {
                    if execute_block(body, register_count, registers, state, steps, ceiling)? { return Ok(true); }
                }
            }
            _ => return Err(()),
        }
    }
    Ok(false)
}

fn oracle(script: &[u8]) -> Result<(OracleState, u32), ()> {
    if script.len() < 10 || script.len() > 4_096 || &script[..4] != b"LXSI" || script[4] != 1 { return Err(()); }
    let registers = usize::from(script[5]);
    if registers == 0 || registers > 16 { return Err(()); }
    let ceiling = u32::from(u16::from_be_bytes([script[6], script[7]]));
    let length = usize::from(u16::from_be_bytes([script[8], script[9]]));
    if ceiling == 0 || ceiling > 4_096 || length == 0 || 10_usize.checked_add(length) != Some(script.len()) { return Err(()); }
    let code = &script[10..];
    let mut remaining = ceiling;
    inspect(code, registers, 0, &mut remaining)?;
    let mut state = OracleState::default();
    let mut values = [0_i64; 16];
    let mut steps = 0;
    execute_block(code, registers, &mut values, &mut state, &mut steps, ceiling)?;
    Ok((state, steps))
}

#[test]
fn fixed_opcode_oracle_freezes_every_success_effect_and_step() {
    let scripts = vectors(SUCCESSES);
    assert_eq!(scripts.len(), 4);
    let expected = [
        (OracleState { storage: vec![(b"sum".to_vec(), 12)], transfers: vec![] }, 5),
        (OracleState { storage: vec![], transfers: vec![([1; 32], [2; 32], 8)] }, 17),
        (OracleState {
            storage: vec![
                (b"sub".to_vec(), 6), (b"mul".to_vec(), 27), (b"div".to_vec(), 3),
                (b"eq".to_vec(), 0), (b"lt".to_vec(), 1),
            ],
            transfers: vec![],
        }, 13),
        (OracleState::default(), 5),
    ];
    for (script, expected) in scripts.iter().zip(expected) {
        Interpreter::validate(script).unwrap_or_else(|error| panic!("production validation: {error}"));
        assert_eq!(oracle(script), Ok(expected));
    }
}

#[test]
fn fixed_refusals_cover_structure_arithmetic_amount_and_depth_without_effects() {
    let scripts = vectors(REFUSALS);
    assert_eq!(scripts.len(), 8);
    for (index, script) in scripts.iter().enumerate() {
        let production_submission = Interpreter::validate(script);
        if matches!(index, 0 | 1 | 6) {
            assert!(production_submission.is_err());
        } else {
            production_submission.unwrap_or_else(|error| panic!("runtime refusal vector {index}: {error}"));
        }
        assert!(oracle(script).is_err());
    }
}
