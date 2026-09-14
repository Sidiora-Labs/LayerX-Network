pub(super) use layerx_wire::native_budget::BudgetRecord as Record;

#[cfg(test)]
mod tests {
    use super::Record;

    fn vectors() -> [Vec<u8>; 2] {
        let value: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../tests/vectors/native-budget-records.json"
        )))
        .unwrap_or_else(|error| panic!("native vectors: {error}"));
        ["v1", "v2"].map(|key| {
            crate::hex::decode(
                value[key]
                    .as_str()
                    .unwrap_or_else(|| panic!("{key}"))
                    .strip_prefix("0x")
                    .unwrap_or_else(|| panic!("hex prefix")),
            )
            .unwrap_or_else(|_| panic!("canonical native hex"))
        })
    }

    #[test]
    fn actual_native_versions_bind_source_revocation_and_remaining() {
        for (index, bytes) in vectors().iter().enumerate() {
            let record = Record::decode(bytes).unwrap_or_else(|_| panic!("native record"));
            assert_eq!(record.id, [1; 32]);
            assert_eq!(record.owner, [2; 32]);
            assert_eq!(record.account, [3; 32]);
            assert_eq!(record.asset, [4; 32]);
            assert_eq!(record.source, (index == 1).then_some([8; 32]));
            assert_eq!(record.revocation, 31);
            assert_eq!(record.remaining(500, 1000, false), 77);
            assert_eq!(record.remaining(50, 8999, false), 50);
            assert_eq!(record.remaining(500, 9000, false), 0);
            assert_eq!(record.remaining(500, 999, false), 0);
            assert_eq!(record.remaining(500, 1000, true), 0);
        }
    }

    #[test]
    fn native_record_decoder_refuses_noncanonical_encodings() {
        for bytes in vectors() {
            for end in 0..bytes.len() {
                assert!(Record::decode(&bytes[..end]).is_err(), "truncation {end}");
            }
            let mut trailing = bytes.clone();
            trailing.push(0);
            assert!(Record::decode(&trailing).is_err());
            for (offset, value) in [
                (0, 1),
                (1, 3),
                (274, 0),
                (274, 3),
                (275, 2),
                (276, 2),
                (277, 17),
            ] {
                let mut changed = bytes.clone();
                changed[offset] = value;
                assert!(Record::decode(&changed).is_err(), "field {offset}");
            }
            for (offset, length) in [(2, 32), (162, 16), (242, 8), (258, 8)] {
                let mut changed = bytes.clone();
                changed[offset..offset + length].fill(0);
                assert!(Record::decode(&changed).is_err(), "zero field {offset}");
            }
            let mut duplicate = bytes.clone();
            duplicate.copy_within(278..310, 310);
            assert!(Record::decode(&duplicate).is_err());
            let mut unsorted = bytes.clone();
            unsorted[278..310].fill(9);
            assert!(Record::decode(&unsorted).is_err());
            let mut carry = bytes.clone();
            carry[209] = 1;
            assert!(Record::decode(&carry).is_err());
            if bytes[1] == 2 {
                let mut source = bytes.clone();
                source[342..374].fill(0);
                assert!(Record::decode(&source).is_err());
            }
        }
    }

    #[test]
    fn exhausted_closed_and_revoked_budgets_never_report_spendable_value() {
        let [_, bytes] = vectors();
        let mut record = Record::decode(&bytes).unwrap_or_else(|_| panic!("native record"));
        record.spent = record.limit;
        assert_eq!(record.remaining(u128::MAX, 2000, false), 0);
        record.spent = u128::MAX;
        assert_eq!(record.remaining(u128::MAX, 2000, false), 0);
        record.spent = 0;
        record.closed = true;
        assert_eq!(record.remaining(u128::MAX, 2000, false), 0);
        record.closed = false;
        record.revoked = true;
        assert_eq!(record.remaining(u128::MAX, 2000, false), 0);
    }
}
