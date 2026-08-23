# LayerX Calldata Encoding Golden Vectors

This directory contains frozen golden test vectors for the LayerX canonical calldata encoding convention.

## Structure

- `valid/` - Valid canonical encodings that MUST decode successfully
- `invalid/` - Invalid or non-canonical encodings that MUST be rejected
- `boundaries/` - Boundary cases for size limits and nesting depth
- `evm/` - EVM head-only convention vectors

## Vector Format

Each `.json` file contains:
```json
{
  "description": "Human-readable description",
  "hex": "Hex-encoded bytes",
  "expected": "pass" | "reject",
  "error": "Expected error code (for reject cases)"
}
```

## Conventions

1. **Canonical Requirement**: Only one valid encoding exists per logical value
2. **Nesting Limit**: Maximum depth of 16 nested structures
3. **Size Limits**: Input ≤ 1 MiB, decoded ≤ 16 MiB
4. **Byte Order**: All multi-byte integers use big-endian encoding

## Coverage

- All primitive integer types (u8, u16, u32, u64, u128, u256, i8, i16, i32, i64, i128)
- Byte strings (empty, typical, maximum)
- Fixed and variable arrays (empty, nested, maximum depth)
- Options (None, Some with nested values)
- Tagged unions (all variant indices)
- EVM head-only layout (32-byte aligned words)
- Rejection cases (truncated, non-canonical, oversized, invalid tags)
