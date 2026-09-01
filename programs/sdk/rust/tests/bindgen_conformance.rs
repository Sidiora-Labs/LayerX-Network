use layerx_program_sdk::{BindgenError, BindingGenerator};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

const DOMAIN: &[u8] = b"LayerX/program-interface/v1\0";
const CODE_HASH: [u8; 32] = [0x5a; 32];

fn push_text(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(&u16::try_from(value.len()).unwrap_or_else(|error| panic!("fixture text length: {error}" )).to_be_bytes());
    out.extend_from_slice(value.as_bytes());
}

fn layerx(tag: u8) -> Vec<u8> {
    vec![1, tag]
}

fn entry(out: &mut Vec<u8>, name: &str, discriminator: [u8; 4], schema: &[u8]) {
    push_text(out, name);
    out.extend_from_slice(&discriminator);
    out.extend_from_slice(schema);
    out.extend_from_slice(schema);
    out.extend_from_slice(&0_u16.to_be_bytes());
    out.extend_from_slice(&0_u16.to_be_bytes());
    out.extend_from_slice(&1_u16.to_be_bytes());
    out.extend_from_slice(&7_u32.to_be_bytes());
    push_text(out, "refused");
    out.extend_from_slice(schema);
}

fn infallible_entry(out: &mut Vec<u8>, name: &str, discriminator: [u8; 4]) {
    push_text(out, name);
    out.extend_from_slice(&discriminator);
    out.extend_from_slice(&layerx(0x10));
    out.extend_from_slice(&layerx(0x10));
    out.extend_from_slice(&0_u16.to_be_bytes());
    out.extend_from_slice(&0_u16.to_be_bytes());
    out.extend_from_slice(&0_u16.to_be_bytes());
}

fn difficult_names_interface() -> Vec<u8> {
    let mut names = vec![
        ("1entry", [0x10, 0, 0, 1]), ("Alpha", [0x10, 0, 0, 2]),
        ("_entry", [0x10, 0, 0, 3]), ("alpha", [0x10, 0, 0, 4]),
        ("fooBar", [0x10, 0, 0, 5]), ("foo_bar", [0x10, 0, 0, 6]),
        ("match", [0x10, 0, 0, 7]),
    ];
    names.sort_by_key(|(name, _)| *name);
    let mut out = Vec::new();
    out.extend_from_slice(DOMAIN);
    out.extend_from_slice(&CODE_HASH);
    out.extend_from_slice(&2_u16.to_be_bytes());
    out.extend_from_slice(&u16::try_from(names.len()).unwrap_or_else(|error| panic!("difficult-name count: {error}")).to_be_bytes());
    for (name, discriminator) in names { infallible_entry(&mut out, name, discriminator); }
    out
}

fn exhaustive_interface() -> Vec<u8> {
    let mut schemas = vec![
        ("bytes", { let mut v = layerx(0x20); v.extend_from_slice(&8_u32.to_be_bytes()); v }),
        ("evm", vec![2]),
        ("fixed", { let mut v = layerx(0x30); v.extend_from_slice(&2_u32.to_be_bytes()); v.push(0x10); v }),
        ("i128", layerx(0x1c)), ("i16", layerx(0x19)), ("i32", layerx(0x1a)),
        ("i64", layerx(0x1b)), ("i8", layerx(0x18)),
        ("option", { let mut v = layerx(0x40); v.push(0x12); v }),
        ("u128", layerx(0x14)), ("u16", layerx(0x11)), ("u256", layerx(0x15)),
        ("u32", layerx(0x12)), ("u64", layerx(0x13)), ("u8", layerx(0x10)),
        ("union", { let mut v = layerx(0x50); v.extend_from_slice(&2_u16.to_be_bytes()); v.extend_from_slice(&0_u32.to_be_bytes()); v.push(0x10); v.extend_from_slice(&7_u32.to_be_bytes()); v.push(0x11); v }),
        ("variable", { let mut v = layerx(0x31); v.extend_from_slice(&3_u32.to_be_bytes()); v.push(0x11); v }),
    ];
    schemas.sort_by_key(|(name, _)| *name);
    let mut out = Vec::new();
    out.extend_from_slice(DOMAIN);
    out.extend_from_slice(&CODE_HASH);
    out.extend_from_slice(&2_u16.to_be_bytes());
    out.extend_from_slice(&u16::try_from(schemas.len()).unwrap_or_else(|error| panic!("fixture entry count: {error}")).to_be_bytes());
    for (index, (name, schema)) in schemas.iter().enumerate() {
        entry(&mut out, name, [0xa5, 0, 0, u8::try_from(index + 1).unwrap_or_else(|error| panic!("fixture discriminator: {error}"))], schema);
    }
    out
}

#[test]
fn canonical_fixture_generates_digest_bound_self_contained_artifacts() {
    let bytes = exhaustive_interface();
    let expected_digest: [u8; 32] = Sha256::digest(&bytes).into();
    let generator = BindingGenerator::from_interface(&bytes).unwrap_or_else(|error| panic!("canonical fixture refused: {error}"));
    assert_eq!(generator.interface_digest(), expected_digest);
    assert_eq!(generator.code_hash(), CODE_HASH);
    assert_eq!(generator.require_digest(expected_digest), Ok(()));
    assert_eq!(generator.require_code_hash(CODE_HASH), Ok(()));
    assert_eq!(
        generator.require_digest([0x33; 32]),
        Err(BindgenError::StaleBinding { expected: expected_digest, published: [0x33; 32] })
    );
    assert_eq!(
        generator.require_code_hash([0x44; 32]),
        Err(BindgenError::CodeHashMismatch { expected: CODE_HASH, deployed: [0x44; 32] })
    );

    let artifacts = generator.generate_all();
    assert_eq!(artifacts.interface_digest, expected_digest);
    for required in ["u8", "u16", "u32", "u64", "u128", "u256", "i8", "i16", "i32", "i64", "i128", "bytes", "fixed", "variable", "option", "union", "evm"] {
        assert!(artifacts.rust.contains(&format!("pub mod {required}")));
        assert!(artifacts.guest.contains(&format!("pub mod {required}")));
    }
    assert!(artifacts.rust.contains("check_target(deployed_code_hash,published_digest)?"));
    assert!(artifacts.typescript.contains("checkTarget(deployedCodeHash,publishedDigest);"));
    assert!(artifacts.guest.contains("pub fn dispatch<P:Program>"));
    assert!(artifacts.typescript.contains("export type UnionInput = {tag:0;value:number} | {tag:7;value:number};"));
}

#[test]
fn frozen_vectors_are_exact_and_shared_by_both_clients() {
    let generated = BindingGenerator::from_interface(&exhaustive_interface()).unwrap_or_else(|error| panic!("canonical fixture refused: {error}")).generate_all();
    let vectors = [
        ("u8", "[16, 127]"),
        ("u16", "[17, 18, 52]"),
        ("some", "[64, 1, 18, 0, 0, 0, 8]"),
        ("union7", "[80, 0, 0, 0, 7, 17, 0, 10]"),
        ("evm", "[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]"),
    ];
    for (name, exact) in vectors {
        assert!(generated.rust.contains(&format!("(\"{name}\",&{exact})")));
        assert!(generated.typescript.contains(&format!("['{name}',Uint8Array.from({exact})]")));
    }
}

#[test]
fn generated_sources_expose_every_roundtrip_and_failure_path() {
    let generated = BindingGenerator::from_interface(&exhaustive_interface()).unwrap_or_else(|error| panic!("canonical fixture refused: {error}")).generate_all();
    for name in ["Bytes", "Evm", "Fixed", "I128", "I16", "I32", "I64", "I8", "Option", "U128", "U16", "U256", "U32", "U64", "U8", "Union", "Variable"] {
        assert!(generated.typescript.contains(&format!("export function encode{name}(")));
        assert!(generated.typescript.contains(&format!("export function decode{name}Output(")));
        assert!(generated.typescript.contains(&format!("export function decode{name}Failure(")));
    }
    for name in ["bytes", "evm", "fixed", "i128", "i16", "i32", "i64", "i8", "option", "u128", "u16", "u256", "u32", "u64", "u8", "union_binding", "variable"] {
        assert!(generated.rust.contains(&format!("pub fn decode_output(bytes:&[u8])")));
        assert!(generated.rust.contains(&format!("pub mod {name}")));
        assert!(generated.guest.contains(&format!("fn {name}(&mut self, input:")));
    }
    assert_eq!(generated.rust.matches("pub fn decode_failure(code:u32").count(), 17);
    assert_eq!(generated.guest.matches("Err(DispatchFailure::Typed{code,detail})").count(), 17);
    assert!(!generated.rust.contains("pub type Input=Input"));
    assert!(!generated.rust.contains("pub type Output=Output"));
    assert!(!generated.guest.contains("pub type Input=Input"));
    assert!(!generated.guest.contains("pub type Output=Output"));
}

#[test]
fn infallible_and_difficult_names_have_stable_collision_free_symbols() {
    let generated = BindingGenerator::from_interface(&difficult_names_interface()).unwrap_or_else(|error| panic!("difficult names refused: {error}")).generate_all();
    for rust_name in ["n_1entry", "alpha_lx_10000002", "_entry", "alpha_lx_10000004", "foobar", "foo_bar", "match_binding"] {
        assert!(generated.rust.contains(&format!("pub mod {rust_name}")), "missing Rust symbol {rust_name}");
        assert!(generated.guest.contains(&format!("pub mod {rust_name}")), "missing guest symbol {rust_name}");
    }
    for ts_name in ["N1entry", "AlphaLx10000002", "Entry", "AlphaLx10000004", "FooBarLx10000005", "FooBarLx10000006", "Match"] {
        assert!(generated.typescript.contains(&format!("export type {ts_name}Failure = never;")), "missing TypeScript symbol {ts_name}");
    }
    assert_eq!(generated.rust.matches("pub type Failure=core::convert::Infallible;").count(), 7);
    assert_eq!(generated.guest.matches("pub type Failure=core::convert::Infallible;").count(), 7);
    assert_eq!(generated.typescript.matches("Failure = never;").count(), 7);
}

#[test]
#[ignore = "human qualification invokes the installed Rust and TypeScript compilers"]
fn generated_client_guest_and_typescript_are_compiler_inputs() {
    let generated = BindingGenerator::from_interface(&exhaustive_interface()).unwrap_or_else(|error| panic!("canonical fixture refused: {error}")).generate_all();
    let root = std::env::temp_dir().join(format!("layerx-bindgen-conformance-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap_or_else(|error| panic!("create conformance directory: {error}"));
    let client = root.join("client.rs");
    let guest = root.join("guest.rs");
    let typescript = root.join("bindings.ts");
    let rust_consumer = r#"
fn encoded<T:CanonicalEncode>(value:&T)->Vec<u8>{let mut out=Vec::new();value.encode(&mut out).unwrap_or_else(|error|panic!("encode: {error:?}"));out}
fn main(){
 assert_eq!(encoded(&127u8),vec![0x10,0x7f]);assert_eq!(encoded(&0x1234u16),vec![0x11,0x12,0x34]);
 assert_eq!(encoded(&7u32),vec![0x12,0,0,0,7]);assert_eq!(encoded(&9u64),vec![0x13,0,0,0,0,0,0,0,9]);
 assert_eq!(encoded(&11u128),vec![0x14,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,11]);
 assert_eq!(encoded(&U256({let mut v=[0;32];v[31]=13;v})),FROZEN_CODEC_VECTORS[5].1);
 assert_eq!(encoded(&-1i8),vec![0x18,0xff]);assert_eq!(encoded(&-2i16),vec![0x19,0xff,0xfe]);
 assert_eq!(encoded(&-3i32),vec![0x1a,0xff,0xff,0xff,0xfd]);assert_eq!(encoded(&-4i64),FROZEN_CODEC_VECTORS[9].1);assert_eq!(encoded(&-5i128),FROZEN_CODEC_VECTORS[10].1);
 let bytes=BoundedBytes::<8>::new(vec![1,2,3]).unwrap_or_else(|e|panic!("bytes: {e:?}"));assert_eq!(encoded(&bytes),FROZEN_CODEC_VECTORS[11].1);
 let fixed=FixedArray::<u8,2>::new(vec![1,2]).unwrap_or_else(|e|panic!("fixed: {e:?}"));assert_eq!(encoded(&fixed),FROZEN_CODEC_VECTORS[12].1);
 let variable=BoundedVec::<u16,3>::new(vec![1,2]).unwrap_or_else(|e|panic!("variable: {e:?}"));assert_eq!(encoded(&variable),FROZEN_CODEC_VECTORS[13].1);
 assert_eq!(encoded(&Option::<u32>::None),FROZEN_CODEC_VECTORS[14].1);assert_eq!(encoded(&Some(8u32)),FROZEN_CODEC_VECTORS[15].1);
 let union0=union_binding::Input::Variant0(9);let union7=union_binding::Input::Variant1(10);assert_eq!(encoded(&union0),FROZEN_CODEC_VECTORS[16].1);assert_eq!(encoded(&union7),FROZEN_CODEC_VECTORS[17].1);
 let evm=EvmHead::new({let mut v=vec![0;32];v[31]=1;v}).unwrap_or_else(|e|panic!("evm: {e:?}"));assert_eq!(encoded(&evm),FROZEN_CODEC_VECTORS[18].1);
 let values=(bytes::call(&bytes,CODE_HASH,INTERFACE_DIGEST),evm::call(&evm,CODE_HASH,INTERFACE_DIGEST),fixed::call(&fixed,CODE_HASH,INTERFACE_DIGEST),i128::call(&-5,CODE_HASH,INTERFACE_DIGEST),i16::call(&-2,CODE_HASH,INTERFACE_DIGEST),i32::call(&-3,CODE_HASH,INTERFACE_DIGEST),i64::call(&-4,CODE_HASH,INTERFACE_DIGEST),i8::call(&-1,CODE_HASH,INTERFACE_DIGEST),option::call(&Some(8),CODE_HASH,INTERFACE_DIGEST),u128::call(&11,CODE_HASH,INTERFACE_DIGEST),u16::call(&0x1234,CODE_HASH,INTERFACE_DIGEST),u256::call(&U256([0;32]),CODE_HASH,INTERFACE_DIGEST),u32::call(&7,CODE_HASH,INTERFACE_DIGEST),u64::call(&9,CODE_HASH,INTERFACE_DIGEST),u8::call(&127,CODE_HASH,INTERFACE_DIGEST),union_binding::call(&union0,CODE_HASH,INTERFACE_DIGEST),variable::call(&variable,CODE_HASH,INTERFACE_DIGEST));
 let _=values;
 assert!(matches!(u8::call(&127,[0;32],INTERFACE_DIGEST),Err(BindingRefusal::CodeHashMismatch)));
 assert!(matches!(u8::call(&127,CODE_HASH,[0;32]),Err(BindingRefusal::StaleInterface)));
 assert!(matches!(u8::decode_failure(7,&[1,0x10,0x7f]),Ok(u8::Failure::Refused(127))));
 assert_eq!(u8::decode_output(&[1,0x10,0x7f]),Ok(127));
}
"#;
    fs::write(&client, format!("{}{}", generated.rust, rust_consumer)).unwrap_or_else(|error| panic!("write generated client consumer: {error}"));
    let guest_consumer = r#"
struct ConformanceProgram;
impl Program for ConformanceProgram {
 fn bytes(&mut self,v:bytes::Input)->Result<bytes::Output,bytes::Failure>{Ok(v)} fn evm(&mut self,v:evm::Input)->Result<evm::Output,evm::Failure>{Ok(v)}
 fn fixed(&mut self,v:fixed::Input)->Result<fixed::Output,fixed::Failure>{Ok(v)} fn i128(&mut self,v:i128::Input)->Result<i128::Output,i128::Failure>{Ok(v)}
 fn i16(&mut self,v:i16::Input)->Result<i16::Output,i16::Failure>{Ok(v)} fn i32(&mut self,v:i32::Input)->Result<i32::Output,i32::Failure>{Ok(v)}
 fn i64(&mut self,v:i64::Input)->Result<i64::Output,i64::Failure>{Ok(v)} fn i8(&mut self,v:i8::Input)->Result<i8::Output,i8::Failure>{Ok(v)}
 fn option(&mut self,v:option::Input)->Result<option::Output,option::Failure>{Ok(v)} fn u128(&mut self,v:u128::Input)->Result<u128::Output,u128::Failure>{Ok(v)}
 fn u16(&mut self,v:u16::Input)->Result<u16::Output,u16::Failure>{Ok(v)} fn u256(&mut self,v:u256::Input)->Result<u256::Output,u256::Failure>{Ok(v)}
 fn u32(&mut self,v:u32::Input)->Result<u32::Output,u32::Failure>{Ok(v)} fn u64(&mut self,v:u64::Input)->Result<u64::Output,u64::Failure>{Ok(v)}
 fn u8(&mut self,v:u8::Input)->Result<u8::Output,u8::Failure>{Err(u8::Failure::Refused(v))}
 fn union_binding(&mut self,v:union_binding::Input)->Result<union_binding::Output,union_binding::Failure>{match v{union_binding::Input::Variant0(value)=>Ok(union_binding::Output::Variant0(value)),union_binding::Input::Variant1(value)=>Ok(union_binding::Output::Variant1(value))}} fn variable(&mut self,v:variable::Input)->Result<variable::Output,variable::Failure>{Ok(v)}
}
fn main(){let mut p=ConformanceProgram;assert_eq!(dispatch(&mut p,&[0xa5,0,0,15,1,0x10,0x7f]),Err(DispatchFailure::Typed{code:7,detail:vec![1,0x10,0x7f]}));assert!(dispatch(&mut p,&[0xa5,0,0,16,1,0x50,0,0,0,0,0x10,9]).is_ok());assert!(dispatch(&mut p,&[0xa5,0,0,16,1,0x50,0,0,0,7,0x11,0,10]).is_ok());}
"#;
    fs::write(&guest, format!("{}{}", generated.guest, guest_consumer)).unwrap_or_else(|error| panic!("write generated guest consumer: {error}"));
    let ts_consumer = r#"
const hash=CODE_HASH,digest=INTERFACE_DIGEST;
const bytesValue=boundedBytes(8,Uint8Array.of(1,2,3)),fixedValue=fixedArray(2,[1,2]),variableValue=variableArray(3,[1,2]);
const evmValue=evmHead(Uint8Array.from([...new Uint8Array(31),1]));
const calls=[encodeBytes(bytesValue,hash,digest),encodeEvm(evmValue,hash,digest),encodeFixed(fixedValue,hash,digest),encodeI128(-5n,hash,digest),encodeI16(-2,hash,digest),encodeI32(-3,hash,digest),encodeI64(-4n,hash,digest),encodeI8(-1,hash,digest),encodeOption(8,hash,digest),encodeU128(11n,hash,digest),encodeU16(0x1234,hash,digest),encodeU256(new Uint8Array(32),hash,digest),encodeU32(7,hash,digest),encodeU64(9n,hash,digest),encodeU8(127,hash,digest),encodeUnion({tag:0,value:9},hash,digest),encodeUnion({tag:7,value:10},hash,digest),encodeVariable(variableValue,hash,digest)];
const protectedCall=encodeU8(127,hash,digest),leakedBytes=protectedCall.bytes;leakedBytes[0]=0;if(protectedCall.bytes[0]!==0xa5)throw new Error('call bytes were mutated through an accessor');
decodeU8Output(Uint8Array.of(1,0x10,0x7f));decodeU8Failure(7,Uint8Array.of(1,0x10,0x7f));
try{encodeU8(127,'00'.repeat(32),digest);throw new Error('missing code-hash refusal')}catch(error){if(!(error instanceof BindingRefusal)||error.code!=='CODE_HASH_MISMATCH')throw error;}
try{encodeU8(127,hash,'00'.repeat(32));throw new Error('missing stale refusal')}catch(error){if(!(error instanceof BindingRefusal)||error.code!=='STALE_INTERFACE')throw error;}
// @ts-expect-error LayerXCall is branded and cannot be forged by arbitrary transport bytes.
const forged:LayerXCall<U8Output,U8Failure>={bytes:Uint8Array.of(1)};
void calls;void forged;
"#;
    fs::write(&typescript, format!("{}{}", generated.typescript, ts_consumer)).unwrap_or_else(|error| panic!("write generated TypeScript consumer: {error}"));

    for source in [&client, &guest] {
        let output = Command::new("rustc").args(["--edition=2021"]).arg(source).arg("--out-dir").arg(&root).output().unwrap_or_else(|error| panic!("invoke rustc for {}: {error}", source.display()));
        assert!(output.status.success(), "rustc rejected {}:\n{}", source.display(), String::from_utf8_lossy(&output.stderr));
        let executable = root.join(source.file_stem().unwrap_or_else(|| panic!("source has no stem")));
        let output = Command::new(&executable).output().unwrap_or_else(|error| panic!("run generated conformance consumer {}: {error}", executable.display()));
        assert!(output.status.success(), "generated conformance consumer {} failed:\n{}", executable.display(), String::from_utf8_lossy(&output.stderr));
    }
    let typescript_compiler = Path::new(env!("CARGO_MANIFEST_DIR")).join("node_modules/.bin/tsc");
    let output = Command::new(&typescript_compiler)
        .args(["--strict", "--target", "ES2020", "--module", "commonjs", "--outDir"])
        .arg(&root)
        .arg(&typescript)
        .output()
        .unwrap_or_else(|error| panic!("invoke TypeScript compiler {}: {error}", typescript_compiler.display()));
    assert!(output.status.success(), "tsc rejected generated bindings:\n{}", String::from_utf8_lossy(&output.stderr));
    let output = Command::new("node").arg(root.join("bindings.js")).output().unwrap_or_else(|error| panic!("run generated TypeScript consumer: {error}"));
    assert!(output.status.success(), "generated TypeScript consumer failed:\n{}", String::from_utf8_lossy(&output.stderr));
}
