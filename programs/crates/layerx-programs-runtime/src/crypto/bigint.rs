//! Wide-integer and modular exponentiation host functions.

use wasmi::{Caller, Linker};

use crate::execute::ExecutionFault;

use super::super::host::memory::{nonnegative, read_guest, write_guest};
use super::super::host::{linker_fault, RuntimeState};
use crate::abi::ABI_V2_MODULE;

const STATUS_BOUNDS: i32 = -3;
const STATUS_METER: i32 = -4;
const STATUS_INVALID: i32 = -2;

const MAX_OPERAND_WIDTH: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WideIntegerOp {
    Mul,
    Div,
    Rem,
    ModExp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WideIntegerRefusal {
    pub op: WideIntegerOp,
    pub reason: WideIntegerRefusalReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WideIntegerRefusalReason {
    DivisionByZero,
    MalformedWidth,
    ModulusZero,
}

fn bigint_operation_fuel(
    op: WideIntegerOp,
    operand_width: usize,
    exponent_bits: Option<usize>,
) -> u64 {
    let base_cost = match op {
        WideIntegerOp::Mul => operand_width.saturating_mul(2),
        WideIntegerOp::Div | WideIntegerOp::Rem => operand_width.saturating_mul(4),
        WideIntegerOp::ModExp => {
            let exp_bits = exponent_bits.unwrap_or(0);
            operand_width
                .saturating_mul(8)
                .saturating_add(exp_bits.saturating_mul(operand_width))
        }
    };
    
    u64::try_from(base_cost).unwrap_or(u64::MAX)
}

fn bigint_mul_256(a: &[u8; 32], b: &[u8; 32]) -> [u8; 64] {
    let mut result = [0u8; 64];
    
    for i in 0..32 {
        let mut carry = 0u16;
        for j in 0..32 {
            let pos = i + j;
            let product = u16::from(a[31 - i])
                .wrapping_mul(u16::from(b[31 - j]))
                .wrapping_add(u16::from(result[63 - pos]))
                .wrapping_add(carry);
            result[63 - pos] = (product & 0xFF) as u8;
            carry = product >> 8;
        }
        if i + 32 < 64 {
            result[63 - (i + 32)] = result[63 - (i + 32)].wrapping_add(carry as u8);
        }
    }
    
    result
}

fn bigint_from_be_bytes(bytes: &[u8]) -> num_bigint::BigUint {
    num_bigint::BigUint::from_bytes_be(bytes)
}

fn bigint_to_be_bytes(value: &num_bigint::BigUint, width: usize) -> Vec<u8> {
    let bytes = value.to_bytes_be();
    if bytes.len() >= width {
        bytes[bytes.len() - width..].to_vec()
    } else {
        let mut padded = vec![0u8; width - bytes.len()];
        padded.extend_from_slice(&bytes);
        padded
    }
}

fn bigint_div_256(a: &[u8; 32], b: &[u8; 32]) -> Result<[u8; 32], WideIntegerRefusal> {
    let divisor = bigint_from_be_bytes(b);
    
    if divisor == num_bigint::BigUint::ZERO {
        return Err(WideIntegerRefusal {
            op: WideIntegerOp::Div,
            reason: WideIntegerRefusalReason::DivisionByZero,
        });
    }
    
    let dividend = bigint_from_be_bytes(a);
    let quotient = dividend / divisor;
    
    let bytes = bigint_to_be_bytes(&quotient, 32);
    let mut result = [0u8; 32];
    result.copy_from_slice(&bytes);
    Ok(result)
}

fn bigint_rem_256(a: &[u8; 32], b: &[u8; 32]) -> Result<[u8; 32], WideIntegerRefusal> {
    let divisor = bigint_from_be_bytes(b);
    
    if divisor == num_bigint::BigUint::ZERO {
        return Err(WideIntegerRefusal {
            op: WideIntegerOp::Rem,
            reason: WideIntegerRefusalReason::DivisionByZero,
        });
    }
    
    let dividend = bigint_from_be_bytes(a);
    let remainder = dividend % divisor;
    
    let bytes = bigint_to_be_bytes(&remainder, 32);
    let mut result = [0u8; 32];
    result.copy_from_slice(&bytes);
    Ok(result)
}

fn count_bits(value: &num_bigint::BigUint) -> usize {
    if value == &num_bigint::BigUint::ZERO {
        return 0;
    }
    usize::try_from(value.bits()).unwrap_or(usize::MAX)
}

fn bigint_modexp_256(
    base: &[u8; 32],
    exponent: &[u8; 32],
    modulus: &[u8; 32],
) -> Result<[u8; 32], WideIntegerRefusal> {
    let m = bigint_from_be_bytes(modulus);
    
    if m == num_bigint::BigUint::ZERO {
        return Err(WideIntegerRefusal {
            op: WideIntegerOp::ModExp,
            reason: WideIntegerRefusalReason::ModulusZero,
        });
    }
    
    let b = bigint_from_be_bytes(base);
    let e = bigint_from_be_bytes(exponent);
    
    let result = b.modpow(&e, &m);
    
    let bytes = bigint_to_be_bytes(&result, 32);
    let mut output = [0u8; 32];
    output.copy_from_slice(&bytes);
    Ok(output)
}

fn read_fixed_256(caller: &Caller<'_, RuntimeState>, ptr: i32, len: i32) -> Result<[u8; 32], i32> {
    if len != 32 {
        return Err(STATUS_INVALID);
    }
    let bytes = read_guest(caller, ptr, len, MAX_OPERAND_WIDTH)?;
    let mut array = [0u8; 32];
    array.copy_from_slice(&bytes);
    Ok(array)
}

pub(crate) fn register(linker: &mut Linker<RuntimeState>) -> Result<(), ExecutionFault> {
    linker
        .func_wrap(
            ABI_V2_MODULE,
            "bigint_mul_256",
            |mut caller: Caller<'_, RuntimeState>,
             a_ptr: i32,
             a_len: i32,
             b_ptr: i32,
             b_len: i32,
             output_ptr: i32,
             output_capacity: i32|
             -> i32 {
                let capacity = match nonnegative(output_capacity) {
                    Ok(cap) => cap,
                    Err(status) => return status,
                };
                
                if capacity < 64 {
                    return STATUS_BOUNDS;
                }
                
                let a = match read_fixed_256(&caller, a_ptr, a_len) {
                    Ok(a) => a,
                    Err(status) => return status,
                };
                
                let b = match read_fixed_256(&caller, b_ptr, b_len) {
                    Ok(b) => b,
                    Err(status) => return status,
                };
                
                if super::super::host::charge_host_cpu(&mut caller, bigint_operation_fuel(WideIntegerOp::Mul, 32, None))
                    .is_err()
                {
                    return STATUS_METER;
                }
                
                let result = bigint_mul_256(&a, &b);
                
                if let Err(status) = write_guest(&mut caller, output_ptr, &result) {
                    return status;
                }
                
                64
            },
        )
        .map_err(|error| linker_fault(&error))?;
    
    linker
        .func_wrap(
            ABI_V2_MODULE,
            "bigint_div_256",
            |mut caller: Caller<'_, RuntimeState>,
             a_ptr: i32,
             a_len: i32,
             b_ptr: i32,
             b_len: i32,
             output_ptr: i32,
             output_capacity: i32|
             -> i32 {
                let capacity = match nonnegative(output_capacity) {
                    Ok(cap) => cap,
                    Err(status) => return status,
                };
                
                if capacity < 32 {
                    return STATUS_BOUNDS;
                }
                
                let a = match read_fixed_256(&caller, a_ptr, a_len) {
                    Ok(a) => a,
                    Err(status) => return status,
                };
                
                let b = match read_fixed_256(&caller, b_ptr, b_len) {
                    Ok(b) => b,
                    Err(status) => return status,
                };
                
                if super::super::host::charge_host_cpu(&mut caller, bigint_operation_fuel(WideIntegerOp::Div, 32, None))
                    .is_err()
                {
                    return STATUS_METER;
                }
                
                let result = match bigint_div_256(&a, &b) {
                    Ok(result) => result,
                    Err(_) => return STATUS_INVALID,
                };
                
                if let Err(status) = write_guest(&mut caller, output_ptr, &result) {
                    return status;
                }
                
                32
            },
        )
        .map_err(|error| linker_fault(&error))?;
    
    linker
        .func_wrap(
            ABI_V2_MODULE,
            "bigint_rem_256",
            |mut caller: Caller<'_, RuntimeState>,
             a_ptr: i32,
             a_len: i32,
             b_ptr: i32,
             b_len: i32,
             output_ptr: i32,
             output_capacity: i32|
             -> i32 {
                let capacity = match nonnegative(output_capacity) {
                    Ok(cap) => cap,
                    Err(status) => return status,
                };
                
                if capacity < 32 {
                    return STATUS_BOUNDS;
                }
                
                let a = match read_fixed_256(&caller, a_ptr, a_len) {
                    Ok(a) => a,
                    Err(status) => return status,
                };
                
                let b = match read_fixed_256(&caller, b_ptr, b_len) {
                    Ok(b) => b,
                    Err(status) => return status,
                };
                
                if super::super::host::charge_host_cpu(&mut caller, bigint_operation_fuel(WideIntegerOp::Rem, 32, None))
                    .is_err()
                {
                    return STATUS_METER;
                }
                
                let result = match bigint_rem_256(&a, &b) {
                    Ok(result) => result,
                    Err(_) => return STATUS_INVALID,
                };
                
                if let Err(status) = write_guest(&mut caller, output_ptr, &result) {
                    return status;
                }
                
                32
            },
        )
        .map_err(|error| linker_fault(&error))?;
    
    linker
        .func_wrap(
            ABI_V2_MODULE,
            "bigint_modexp_256",
            |mut caller: Caller<'_, RuntimeState>,
             base_ptr: i32,
             base_len: i32,
             exp_ptr: i32,
             exp_len: i32,
             mod_ptr: i32,
             mod_len: i32,
             output_ptr: i32,
             output_capacity: i32|
             -> i32 {
                let capacity = match nonnegative(output_capacity) {
                    Ok(cap) => cap,
                    Err(status) => return status,
                };
                
                if capacity < 32 {
                    return STATUS_BOUNDS;
                }
                
                let base = match read_fixed_256(&caller, base_ptr, base_len) {
                    Ok(b) => b,
                    Err(status) => return status,
                };
                
                let exponent = match read_fixed_256(&caller, exp_ptr, exp_len) {
                    Ok(e) => e,
                    Err(status) => return status,
                };
                
                let modulus = match read_fixed_256(&caller, mod_ptr, mod_len) {
                    Ok(m) => m,
                    Err(status) => return status,
                };
                
                let exp_value = bigint_from_be_bytes(&exponent);
                let exp_bits = count_bits(&exp_value);
                
                if super::super::host::charge_host_cpu(&mut caller, bigint_operation_fuel(
                    WideIntegerOp::ModExp,
                    32,
                    Some(exp_bits),
                ))
                .is_err()
                {
                    return STATUS_METER;
                }
                
                let result = match bigint_modexp_256(&base, &exponent, &modulus) {
                    Ok(result) => result,
                    Err(_) => return STATUS_INVALID,
                };
                
                if let Err(status) = write_guest(&mut caller, output_ptr, &result) {
                    return status;
                }
                
                32
            },
        )
        .map_err(|error| linker_fault(&error))?;
    
    Ok(())
}

#[cfg(test)]
mod golden_vectors {
    use super::*;

    fn reference_product(left: &[u8; 32], right: &[u8; 32]) -> [u8; 64] {
        let product = num_bigint::BigUint::from_bytes_be(left)
            * num_bigint::BigUint::from_bytes_be(right);
        let encoded = product.to_bytes_be();
        let mut expected = [0; 64];
        expected[64 - encoded.len()..].copy_from_slice(&encoded);
        expected
    }

    #[test]
    fn mul_identity() {
        let one = [0u8; 32];
        let mut one_val = one;
        one_val[31] = 1;
        
        let value = [0u8; 32];
        let mut value_bytes = value;
        value_bytes[31] = 42;
        
        let result = bigint_mul_256(&one_val, &value_bytes);
        
        assert_eq!(&result[32..64], &value_bytes[..]);
        assert_eq!(&result[0..32], &[0u8; 32]);
    }

    #[test]
    fn mul_overflow_boundary() {
        let max = [0xFFu8; 32];
        
        let two = [0u8; 32];
        let mut two_val = two;
        two_val[31] = 2;
        
        let result = bigint_mul_256(&max, &two_val);

        assert_eq!(result, reference_product(&max, &two_val));
        assert_eq!(&result[..31], &[0; 31]);
        assert_eq!(result[31], 0x01);
        assert_eq!(&result[32..63], &[0xFF; 31]);
        assert_eq!(result[63], 0xFE);
    }

    #[test]
    fn mul_maximum_width() {
        let max = [0xFFu8; 32];
        let result = bigint_mul_256(&max, &max);

        assert_eq!(result, reference_product(&max, &max));
        assert_eq!(&result[..31], &[0xFF; 31]);
        assert_eq!(result[31], 0xFE);
        assert_eq!(&result[32..63], &[0; 31]);
        assert_eq!(result[63], 0x01);
    }

    #[test]
    fn div_identity() {
        let mut value = [0u8; 32];
        value[31] = 42;
        
        let mut one = [0u8; 32];
        one[31] = 1;
        
        let result = bigint_div_256(&value, &one).unwrap();
        assert_eq!(result, value);
    }

    #[test]
    fn div_by_zero() {
        let value = [0u8; 32];
        let mut value_bytes = value;
        value_bytes[31] = 42;
        
        let zero = [0u8; 32];
        
        let result = bigint_div_256(&value_bytes, &zero);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().reason,
            WideIntegerRefusalReason::DivisionByZero
        );
    }

    #[test]
    fn rem_identity() {
        let mut value = [0u8; 32];
        value[31] = 42;
        
        let mut divisor = [0u8; 32];
        divisor[31] = 10;
        
        let result = bigint_rem_256(&value, &divisor).unwrap();
        
        let mut expected = [0u8; 32];
        expected[31] = 2;
        assert_eq!(result, expected);
    }

    #[test]
    fn rem_by_zero() {
        let mut value = [0u8; 32];
        value[31] = 42;
        
        let zero = [0u8; 32];
        
        let result = bigint_rem_256(&value, &zero);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().reason,
            WideIntegerRefusalReason::DivisionByZero
        );
    }

    #[test]
    fn modexp_identity() {
        let mut base = [0u8; 32];
        base[31] = 5;
        
        let mut exp = [0u8; 32];
        exp[31] = 1;
        
        let mut modulus = [0u8; 32];
        modulus[31] = 100;
        
        let result = bigint_modexp_256(&base, &exp, &modulus).unwrap();
        assert_eq!(result, base);
    }

    #[test]
    fn modexp_zero_exponent() {
        let mut base = [0u8; 32];
        base[31] = 5;
        
        let exp = [0u8; 32];
        
        let mut modulus = [0u8; 32];
        modulus[31] = 100;
        
        let result = bigint_modexp_256(&base, &exp, &modulus).unwrap();
        
        let mut expected = [0u8; 32];
        expected[31] = 1;
        assert_eq!(result, expected);
    }

    #[test]
    fn modexp_zero_modulus() {
        let mut base = [0u8; 32];
        base[31] = 5;
        
        let mut exp = [0u8; 32];
        exp[31] = 3;
        
        let zero = [0u8; 32];
        
        let result = bigint_modexp_256(&base, &exp, &zero);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().reason,
            WideIntegerRefusalReason::ModulusZero
        );
    }

    #[test]
    fn modexp_maximum_width() {
        let mut base = [0u8; 32];
        base[31] = 2;
        
        let mut exp = [0u8; 32];
        exp[30] = 0x01;
        
        let max_modulus = [0xFFu8; 32];
        
        let result = bigint_modexp_256(&base, &exp, &max_modulus).unwrap();
        
        assert!(result.iter().any(|&b| b != 0));
    }

    #[test]
    fn modexp_overflow_boundary() {
        let max = [0xFFu8; 32];
        
        let mut exp = [0u8; 32];
        exp[31] = 2;
        
        let mut modulus = [0u8; 32];
        modulus[31] = 100;
        
        let result = bigint_modexp_256(&max, &exp, &modulus).unwrap();
        
        let mut expected = [0u8; 32];
        expected[31] = 25;
        assert_eq!(result, expected);
    }
}
