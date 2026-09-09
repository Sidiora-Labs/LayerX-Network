pub(super) fn validate(ordinal: u16, payload: &[u8]) -> Result<(), ()> {
    if payload.len() < 32 || payload[..32] == [0; 32] {
        return Err(());
    }
    match ordinal {
        5 => {
            let count = payload.get(32..34).ok_or(())?;
            let count = usize::from(u16::from_be_bytes([count[0], count[1]]));
            if count == 0 || count > 256 || payload.len() != 34 + count * 112 {
                return Err(());
            }
        }
        6 => {
            if payload.get(32..37) != Some(b"LXPA1") || payload.get(37..69).ok_or(())? == [0; 32] {
                return Err(());
            }
            let length: [u8; 4] = payload.get(69..73).ok_or(())?.try_into().map_err(|_| ())?;
            let length = u32::from_be_bytes(length);
            if length > 128 || payload.len() != 73 + usize::try_from(length).map_err(|_| ())? {
                return Err(());
            }
        }
        _ => return Err(()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate;

    #[test]
    fn native_transfer_count_and_exact_framing_are_required() {
        for count in [1_u16, 256] {
            let mut payload = vec![1; 34 + usize::from(count) * 112];
            payload[32..34].copy_from_slice(&count.to_be_bytes());
            assert_eq!(validate(5, &payload), Ok(()));
            for end in 0..payload.len() {
                assert!(validate(5, &payload[..end]).is_err());
            }
            payload.push(0);
            assert!(validate(5, &payload).is_err());
        }
        for count in [0_u16, 257, u16::MAX] {
            let mut payload = vec![1; 34];
            payload[32..34].copy_from_slice(&count.to_be_bytes());
            assert!(validate(5, &payload).is_err());
        }
    }

    #[test]
    fn native_account_magic_identifiers_and_seed_bound_are_required() {
        for length in [0_u16, 128] {
            let mut payload = vec![1; 73 + usize::from(length)];
            payload[32..37].copy_from_slice(b"LXPA1");
            payload[69..73].copy_from_slice(&u32::from(length).to_be_bytes());
            assert_eq!(validate(6, &payload), Ok(()));
            for end in 0..payload.len() {
                assert!(validate(6, &payload[..end]).is_err());
            }
            for offset in [0, 37] {
                let mut zero = payload.clone();
                zero[offset..offset + 32].fill(0);
                assert!(validate(6, &zero).is_err());
            }
            let mut bad_magic = payload.clone();
            bad_magic[36] = b'2';
            assert!(validate(6, &bad_magic).is_err());
            assert!(validate(4, &payload).is_err());
            payload.push(0);
            assert!(validate(6, &payload).is_err());
            payload[69..73].copy_from_slice(&129_u32.to_be_bytes());
            assert!(validate(6, &payload).is_err());
        }
    }
}
