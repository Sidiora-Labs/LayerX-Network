//! Deterministic typed-binding generation from the canonical published interface.

use alloc::format;
use alloc::boxed::Box;
use alloc::collections::BTreeSet;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::{self, Display, Write};
use sha2::{Digest, Sha256};

const DOMAIN: &[u8] = b"LayerX/program-interface/v1\0";
const MAX_INTERFACE_BYTES: usize = 952;
const MAX_FIELDS: usize = 256;
const MAX_DEPTH: usize = 16;

/// Refusal returned before any source is emitted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindgenError {
    NonCanonical,
    UnsupportedAbi,
    UnsupportedConvention,
    InvalidSchema,
    CodeHashMismatch { expected: [u8; 32], deployed: [u8; 32] },
    StaleBinding { expected: [u8; 32], published: [u8; 32] },
}

impl Display for BindgenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonCanonical => f.write_str("published interface is not canonically encoded"),
            Self::UnsupportedAbi => f.write_str("published interface uses an unsupported ABI"),
            Self::UnsupportedConvention => f.write_str("published interface uses an unsupported encoding convention"),
            Self::InvalidSchema => f.write_str("published interface contains an invalid schema"),
            Self::CodeHashMismatch { .. } => f.write_str("generated binding targets a different deployed code hash"),
            Self::StaleBinding { .. } => f.write_str("generated binding digest does not match the published interface"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Type {
    U8, U16, U32, U64, U128, U256, I8, I16, I32, I64, I128,
    Bytes(u32), Fixed(Box<Type>, u32), Variable(Box<Type>, u32), Option(Box<Type>),
    Union(Vec<Variant>), EvmHead,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Variant { tag: u32, value: Type }

#[derive(Clone, Debug, Eq, PartialEq)]
struct Failure { code: u32, name: String, detail: Type }

#[derive(Clone, Debug, Eq, PartialEq)]
struct Entry { name: String, discriminator: [u8; 4], input: Type, output: Type, failures: Vec<Failure> }

/// All deterministic artifacts generated from one digest-bound interface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedBindings { pub rust: String, pub typescript: String, pub guest: String, pub interface_digest: [u8; 32] }

/// Parsed canonical interface used by the CLI and build integration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BindingGenerator { digest: [u8; 32], code_hash: [u8; 32], entries: Vec<Entry> }

impl BindingGenerator {
    pub fn from_interface(bytes: &[u8]) -> Result<Self, BindgenError> {
        if bytes.len() > MAX_INTERFACE_BYTES || bytes.get(..DOMAIN.len()) != Some(DOMAIN) { return Err(BindgenError::NonCanonical); }
        let mut cursor = DOMAIN.len();
        let code_hash = take::<32>(bytes, &mut cursor)?;
        if code_hash == [0; 32] { return Err(BindgenError::InvalidSchema); }
        let abi = u16::from_be_bytes(take::<2>(bytes, &mut cursor)?);
        if !matches!(abi, 1 | 2) { return Err(BindgenError::UnsupportedAbi); }
        let count = count(bytes, &mut cursor)?;
        if count == 0 { return Err(BindgenError::InvalidSchema); }
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count { entries.push(parse_entry(bytes, &mut cursor)?); }
        let mut discriminators = BTreeSet::new();
        if cursor != bytes.len()
            || !entries.windows(2).all(|pair| pair[0].name < pair[1].name)
            || entries.iter().any(|entry| !discriminators.insert(entry.discriminator))
        { return Err(BindgenError::NonCanonical); }
        let digest: [u8; 32] = Sha256::digest(bytes).into();
        Ok(Self { digest, code_hash, entries })
    }

    #[must_use] pub const fn interface_digest(&self) -> [u8; 32] { self.digest }
    #[must_use] pub const fn code_hash(&self) -> [u8; 32] { self.code_hash }

    pub fn require_digest(&self, published: [u8; 32]) -> Result<(), BindgenError> {
        if self.digest == published { Ok(()) } else { Err(BindgenError::StaleBinding { expected: self.digest, published }) }
    }

    pub fn require_code_hash(&self, deployed: [u8; 32]) -> Result<(), BindgenError> {
        if self.code_hash == deployed { Ok(()) } else { Err(BindgenError::CodeHashMismatch { expected: self.code_hash, deployed }) }
    }

    #[must_use] pub fn generate_all(&self) -> GeneratedBindings {
        GeneratedBindings { rust: self.generate_rust(), typescript: self.generate_typescript(), guest: self.generate_guest(), interface_digest: self.digest }
    }

    #[must_use] pub fn generate_rust(&self) -> String {
        let mut out = generated_header("Rust", self.digest);
        emit_rust_codec_prelude(&mut out);
        out.push_str("pub const INTERFACE_DIGEST: [u8; 32] = ["); hex_array(&mut out, &self.digest); out.push_str("];\n");
        out.push_str("pub const CODE_HASH: [u8; 32] = ["); hex_array(&mut out, &self.code_hash); out.push_str("];\n");
        out.push_str("#[derive(Clone,Copy,Debug,Eq,PartialEq)]pub enum BindingRefusal{StaleInterface,CodeHashMismatch,Encode(CodecError),Decode(CodecError),UnknownFailure(u32)}\nfn check_target(code:[u8;32],digest:[u8;32])->Result<(),BindingRefusal>{if code!=CODE_HASH{return Err(BindingRefusal::CodeHashMismatch)}if digest!=INTERFACE_DIGEST{return Err(BindingRefusal::StaleInterface)}Ok(())}\n");
        out.push_str("#[derive(Clone,Debug,Eq,PartialEq)] pub struct Call<Output,Failure>{bytes:Vec<u8>,_type:core::marker::PhantomData<fn()->Result<Output,Failure>>}\nimpl<Output,Failure> Call<Output,Failure>{pub fn as_bytes(&self)->&[u8]{&self.bytes}}\n");
        emit_rust_frozen_vectors(&mut out);
        for entry in &self.entries { emit_rust_entry(&mut out, entry, &self.entries); }
        out
    }

    #[must_use] pub fn generate_typescript(&self) -> String {
        let mut out = generated_header("TypeScript", self.digest);
        let _ = writeln!(out, "export const INTERFACE_DIGEST = '{}' as const;", hex(&self.digest));
        let _ = writeln!(out, "export const CODE_HASH = '{}' as const;", hex(&self.code_hash));
        out.push_str(r#"export type BindingRefusalCode = 'CODE_HASH_MISMATCH' | 'STALE_INTERFACE' | 'INVALID_VALUE' | 'NON_CANONICAL' | 'TRUNCATED' | 'TRAILING_BYTES' | 'UNKNOWN_FAILURE';
export class BindingRefusal extends Error { constructor(readonly code:BindingRefusalCode,message:string){super(message);this.name='BindingRefusal';} }
function refuse(code:BindingRefusalCode,message:string):never{throw new BindingRefusal(code,message);}
function normalizedHash(value:string,label:string):string{if(typeof value!=='string')refuse('INVALID_VALUE',`${label} must be a 32-byte hexadecimal string`);const normalized=value.startsWith('0x')||value.startsWith('0X')?value.slice(2):value;if(!/^[0-9a-fA-F]{64}$/.test(normalized))refuse('INVALID_VALUE',`${label} must be exactly 32 hexadecimal bytes`);return normalized.toLowerCase();}
function checkTarget(codeHash:string,interfaceDigest:string):void{if(normalizedHash(codeHash,'deployed code hash')!==CODE_HASH)refuse('CODE_HASH_MISMATCH','binding code hash mismatch');if(normalizedHash(interfaceDigest,'published interface digest')!==INTERFACE_DIGEST)refuse('STALE_INTERFACE','stale interface binding');}
const MAX_CALLDATA_BYTES=1048576;
const DECODED_SIZE_LIMIT=16777216;
function checkedLength(value:number,max:number,label:string):number{if(!Number.isSafeInteger(value)||value<0||value>max)refuse('INVALID_VALUE',`${label} length exceeds ${max}`);return value;}
declare const LAYERX_VALUE:unique symbol;
const LAYERX_CALL:unique symbol=Symbol('LayerXCall');
export type BoundedBytes<N extends number> = Uint8Array & {readonly [LAYERX_VALUE]:{readonly kind:'bytes';readonly max:N}};
export type FixedArray<T,N extends number> = ReadonlyArray<T> & {readonly [LAYERX_VALUE]:{readonly kind:'fixed';readonly length:N}};
export type VariableArray<T,N extends number> = ReadonlyArray<T> & {readonly [LAYERX_VALUE]:{readonly kind:'variable';readonly max:N}};
export type EvmHead = Uint8Array & {readonly [LAYERX_VALUE]:{readonly kind:'evm-head'}};
export function boundedBytes<const N extends number>(max:N,value:Uint8Array):BoundedBytes<N>{if(!(value instanceof Uint8Array))refuse('INVALID_VALUE','byte string must be Uint8Array');checkedLength(value.length,max,'byte string');return Uint8Array.from(value) as BoundedBytes<N>;}
export function fixedArray<T,const N extends number>(length:N,value:ReadonlyArray<T>):FixedArray<T,N>{if(!Array.isArray(value)||value.length!==length)refuse('INVALID_VALUE',`fixed array requires ${length} items`);return Array.from(value) as unknown as FixedArray<T,N>;}
export function variableArray<T,const N extends number>(max:N,value:ReadonlyArray<T>):VariableArray<T,N>{if(!Array.isArray(value))refuse('INVALID_VALUE','variable array must be an array');checkedLength(value.length,max,'variable array');return Array.from(value) as unknown as VariableArray<T,N>;}
export function evmHead(value:Uint8Array):EvmHead{if(!(value instanceof Uint8Array))refuse('INVALID_VALUE','EVM head must be Uint8Array');checkedLength(value.length,MAX_CALLDATA_BYTES-1,'EVM head');if(value.length%32!==0)refuse('INVALID_VALUE','EVM head length must be a multiple of 32 bytes');return Uint8Array.from(value) as EvmHead;}
function concat(...parts:ReadonlyArray<Uint8Array>):Uint8Array{let size=0;for(const part of parts){size+=part.length;if(!Number.isSafeInteger(size)||size>MAX_CALLDATA_BYTES)refuse('INVALID_VALUE','encoded value exceeds protocol limit');}const out=new Uint8Array(size);let at=0;for(const part of parts){out.set(part,at);at+=part.length;}return out;}
function u32(value:number):Uint8Array{const out=new Uint8Array(4);new DataView(out.buffer).setUint32(0,value,false);return out;}
function exactInteger(value:number|bigint,label:string):bigint{if(typeof value==='bigint')return value;if(typeof value!=='number'||!Number.isFinite(value)||!Number.isInteger(value)||!Number.isSafeInteger(value))refuse('INVALID_VALUE',`${label} must be an exact safe integer or bigint`);return BigInt(value);}
function unsigned(tag:number,width:number,value:number|bigint):Uint8Array{const v=exactInteger(value,`unsigned ${width*8}-bit integer`);if(v<0n||v>=(1n<<BigInt(width*8)))refuse('INVALID_VALUE',`unsigned ${width*8}-bit integer out of range`);const out=new Uint8Array(width+1);out[0]=tag;let n=v;for(let i=width;i>0;i--){out[i]=Number(n&255n);n>>=8n;}return out;}
function signed(tag:number,width:number,value:number|bigint):Uint8Array{const v=exactInteger(value,`signed ${width*8}-bit integer`);const bits=BigInt(width*8),min=-(1n<<(bits-1n)),max=(1n<<(bits-1n))-1n;if(v<min||v>max)refuse('INVALID_VALUE',`signed ${width*8}-bit integer out of range`);return unsigned(tag,width,v<0n?v+(1n<<bits):v);}
function exactBytes(value:Uint8Array,length:number,label:string):Uint8Array{if(!(value instanceof Uint8Array)||value.length!==length)refuse('INVALID_VALUE',`${label} requires exactly ${length} bytes`);return Uint8Array.from(value);}
class Reader { private at=0;private decoded=0;constructor(private readonly bytes:Uint8Array){} private raw(n:number):Uint8Array{if(!Number.isSafeInteger(n)||n<0||this.at+n>this.bytes.length)refuse('TRUNCATED','truncated canonical value');const value=this.bytes.subarray(this.at,this.at+n);this.at+=n;return value;} byte():number{return this.raw(1)[0];} take(n:number):Uint8Array{if(this.decoded+n>DECODED_SIZE_LIMIT)refuse('NON_CANONICAL','decoded size exceeds protocol limit');this.decoded+=n;return Uint8Array.from(this.raw(n));} rest():Uint8Array{return this.take(this.bytes.length-this.at);} u32():number{const b=this.raw(4);return new DataView(b.buffer,b.byteOffset,4).getUint32(0,false);} done():void{if(this.at!==this.bytes.length)refuse('TRAILING_BYTES','trailing bytes after canonical value');}}
function tag(reader:Reader,expected:number):void{if(reader.byte()!==expected)refuse('NON_CANONICAL','canonical type tag does not match interface schema');}
function readUnsigned(reader:Reader,expected:number,width:number,asNumber:boolean):number|bigint{tag(reader,expected);let value=0n;for(const b of reader.take(width))value=(value<<8n)|BigInt(b);return asNumber?Number(value):value;}
function readSigned(reader:Reader,expected:number,width:number,asNumber:boolean):number|bigint{let value=readUnsigned(reader,expected,width,false) as bigint;const bits=BigInt(width*8);if((value&(1n<<(bits-1n)))!==0n)value-=1n<<bits;return asNumber?Number(value):value;}
function frame(convention:number,payload:Uint8Array):Uint8Array{if(payload.length+1>MAX_CALLDATA_BYTES)refuse('INVALID_VALUE','calldata exceeds protocol limit');return concat(Uint8Array.of(convention),payload);}
function readerFor(bytes:Uint8Array,convention:number):Reader{if(!(bytes instanceof Uint8Array)||bytes.length===0)refuse('TRUNCATED','missing encoding convention');if(bytes.length>MAX_CALLDATA_BYTES)refuse('NON_CANONICAL','calldata exceeds protocol limit');if(bytes[0]!==convention)refuse('NON_CANONICAL','encoding convention does not match interface schema');return new Reader(bytes.subarray(1));}
function finish<T>(reader:Reader,value:T):T{reader.done();return value;}
"#);
        out.push_str("export type LayerXCall<Output,Failure> = Readonly<{ readonly bytes: Uint8Array; readonly [LAYERX_CALL]:{readonly output:Output;readonly failure:Failure} }>;\nfunction layerXCall<Output,Failure>(bytes:Uint8Array):LayerXCall<Output,Failure>{const encoded=Uint8Array.from(bytes);return Object.freeze({get bytes(){return Uint8Array.from(encoded);},[LAYERX_CALL]:undefined as never});}\n");
        emit_ts_frozen_vectors(&mut out);
        for entry in &self.entries { emit_ts_entry(&mut out, entry, &self.entries); }
        out
    }

    #[must_use] pub fn generate_guest(&self) -> String {
        let mut out = generated_header("guest Rust", self.digest);
        emit_rust_codec_prelude(&mut out);
        out.push_str("pub const INTERFACE_DIGEST:[u8;32]=["); hex_array(&mut out, &self.digest); out.push_str("];\npub const CODE_HASH:[u8;32]=["); hex_array(&mut out, &self.code_hash); out.push_str("];\n");
        out.push_str("pub trait Program {\n");
        for entry in &self.entries {
            let n = entry_ident(entry,&self.entries,false);
            let _ = writeln!(out, "fn {n}(&mut self, input: {n}::Input) -> Result<{n}::Output, {n}::Failure>;");
        }
        out.push_str("}\n");
        for entry in &self.entries { emit_guest_entry(&mut out, entry, &self.entries); }
        out.push_str("pub fn dispatch<P:Program>(program:&mut P,input:&[u8])->Result<Vec<u8>,DispatchFailure>{let (disc,payload)=if input.len()>=4{input.split_at(4)}else{return Err(DispatchFailure::Malformed)};match disc{\n");
        for entry in &self.entries { let n=entry_ident(entry,&self.entries,false); let _=writeln!(out,"{:?}=>{n}::dispatch(program,payload),", entry.discriminator); }
        out.push_str("_=>Err(DispatchFailure::MissingEntry)}}}\n#[derive(Clone,Debug,Eq,PartialEq)]pub enum DispatchFailure{Malformed,MissingEntry,Decode(CodecError),Encode(CodecError),Typed{code:u32,detail:Vec<u8>}}\n");
        out
    }
}

fn parse_entry(bytes: &[u8], cursor: &mut usize) -> Result<Entry, BindgenError> {
    let name = text(bytes, cursor)?; valid_name(&name)?;
    let discriminator = take::<4>(bytes, cursor)?;
    let input = schema(bytes, cursor, 0)?; let output = schema(bytes, cursor, 0)?;
    let mut prior_capability: Option<&[u8]> = None;
    for _ in 0..count(bytes, cursor)? {
        let start = *cursor; skip_capability(bytes, cursor)?;
        let encoded = bytes.get(start..*cursor).ok_or(BindgenError::NonCanonical)?;
        if prior_capability.is_some_and(|prior| prior >= encoded) { return Err(BindgenError::NonCanonical); }
        prior_capability = Some(encoded);
    }
    let mut prior_topic = None;
    for _ in 0..count(bytes, cursor)? {
        let topic = take::<32>(bytes, cursor)?;
        if prior_topic.is_some_and(|prior| prior >= topic) { return Err(BindgenError::NonCanonical); }
        prior_topic = Some(topic);
    }
    let mut failures = Vec::new();
    for _ in 0..count(bytes, cursor)? { let code=u32::from_be_bytes(take::<4>(bytes,cursor)?); let failure_name=text(bytes,cursor)?; valid_name(&failure_name)?; failures.push(Failure{code,name:failure_name,detail:schema(bytes,cursor,0)?}); }
    if !failures.windows(2).all(|p| p[0].code < p[1].code) { return Err(BindgenError::NonCanonical); }
    Ok(Entry { name, discriminator, input, output, failures })
}

fn schema(bytes:&[u8], cursor:&mut usize, depth:usize)->Result<Type,BindgenError>{
    if depth>MAX_DEPTH{return Err(BindgenError::InvalidSchema)}
    match take::<1>(bytes,cursor)?[0]{1=>value_type(bytes,cursor,depth),2=>Ok(Type::EvmHead),_=>Err(BindgenError::UnsupportedConvention)}
}
fn value_type(bytes:&[u8],cursor:&mut usize,depth:usize)->Result<Type,BindgenError>{
    if depth>MAX_DEPTH{return Err(BindgenError::InvalidSchema)}
    Ok(match take::<1>(bytes,cursor)?[0]{
        0x10=>Type::U8,0x11=>Type::U16,0x12=>Type::U32,0x13=>Type::U64,0x14=>Type::U128,0x15=>Type::U256,
        0x18=>Type::I8,0x19=>Type::I16,0x1a=>Type::I32,0x1b=>Type::I64,0x1c=>Type::I128,
        0x20=>{let n=u32::from_be_bytes(take::<4>(bytes,cursor)?);if n==0{return Err(BindgenError::InvalidSchema)}Type::Bytes(n)},
        0x30=>{let n=u32::from_be_bytes(take::<4>(bytes,cursor)?);if n==0{return Err(BindgenError::InvalidSchema)}Type::Fixed(Box::new(value_type(bytes,cursor,depth+1)?),n)},
        0x31=>{let n=u32::from_be_bytes(take::<4>(bytes,cursor)?);if n==0{return Err(BindgenError::InvalidSchema)}Type::Variable(Box::new(value_type(bytes,cursor,depth+1)?),n)},
        0x40=>Type::Option(Box::new(value_type(bytes,cursor,depth+1)?)),
        0x50=>{let n=count(bytes,cursor)?;if n==0{return Err(BindgenError::InvalidSchema)}let mut v=Vec::new();for _ in 0..n{v.push(Variant{tag:u32::from_be_bytes(take::<4>(bytes,cursor)?),value:value_type(bytes,cursor,depth+1)?})}if !v.windows(2).all(|p|p[0].tag<p[1].tag){return Err(BindgenError::NonCanonical)}Type::Union(v)},
        _=>return Err(BindgenError::InvalidSchema),
    })
}

fn skip_capability(bytes:&[u8],cursor:&mut usize)->Result<(),BindgenError>{match take::<1>(bytes,cursor)?[0]{
    0..=4=>{},
    5=>{if take::<32>(bytes,cursor)?==[0;32]{return Err(BindgenError::InvalidSchema)}},
    6=>{let asset=take::<32>(bytes,cursor)?;let to=take::<32>(bytes,cursor)?;let amount=u128::from_be_bytes(take::<16>(bytes,cursor)?);if asset==[0;32]||to==[0;32]||amount==0{return Err(BindgenError::InvalidSchema)}},
    7=>{if take::<32>(bytes,cursor)?==[0;32]{return Err(BindgenError::InvalidSchema)}let n=usize::from(u16::from_be_bytes(take::<2>(bytes,cursor)?));if n==0||n>256{return Err(BindgenError::InvalidSchema)}skip(bytes,cursor,n)?;let source=take::<32>(bytes,cursor)?;let asset=take::<32>(bytes,cursor)?;let to=take::<32>(bytes,cursor)?;let amount=u128::from_be_bytes(take::<16>(bytes,cursor)?);if source==[0;32]||asset==[0;32]||to==[0;32]||amount==0{return Err(BindgenError::InvalidSchema)}},
    8=>{if take::<32>(bytes,cursor)?==[0;32]{return Err(BindgenError::InvalidSchema)}},
    9=>{let account=take::<32>(bytes,cursor)?;let asset=take::<32>(bytes,cursor)?;let receipt=take::<32>(bytes,cursor)?;if account==[0;32]||asset==[0;32]||receipt==[0;32]{return Err(BindgenError::InvalidSchema)}},
    _=>return Err(BindgenError::NonCanonical)}Ok(())}
fn valid_name(name:&str)->Result<(),BindgenError>{if name.is_empty()||name.len()>128||!name.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'){Err(BindgenError::InvalidSchema)}else{Ok(())}}
fn count(bytes:&[u8],cursor:&mut usize)->Result<usize,BindgenError>{let n=usize::from(u16::from_be_bytes(take::<2>(bytes,cursor)?));if n>MAX_FIELDS{Err(BindgenError::InvalidSchema)}else{Ok(n)}}
fn text(bytes:&[u8],cursor:&mut usize)->Result<String,BindgenError>{let n=usize::from(u16::from_be_bytes(take::<2>(bytes,cursor)?));let end=cursor.checked_add(n).ok_or(BindgenError::NonCanonical)?;let s=core::str::from_utf8(bytes.get(*cursor..end).ok_or(BindgenError::NonCanonical)?).map_err(|_|BindgenError::NonCanonical)?.to_string();*cursor=end;Ok(s)}
fn take<const N:usize>(bytes:&[u8],cursor:&mut usize)->Result<[u8;N],BindgenError>{let end=cursor.checked_add(N).ok_or(BindgenError::NonCanonical)?;let v=bytes.get(*cursor..end).ok_or(BindgenError::NonCanonical)?.try_into().map_err(|_|BindgenError::NonCanonical)?;*cursor=end;Ok(v)}
fn skip(bytes:&[u8],cursor:&mut usize,n:usize)->Result<(),BindgenError>{let end=cursor.checked_add(n).ok_or(BindgenError::NonCanonical)?;bytes.get(*cursor..end).ok_or(BindgenError::NonCanonical)?;*cursor=end;Ok(())}

fn rust_type(t:&Type)->String{match t{Type::U8=>"u8".into(),Type::U16=>"u16".into(),Type::U32=>"u32".into(),Type::U64=>"u64".into(),Type::U128=>"u128".into(),Type::U256=>"U256".into(),Type::I8=>"i8".into(),Type::I16=>"i16".into(),Type::I32=>"i32".into(),Type::I64=>"i64".into(),Type::I128=>"i128".into(),Type::Bytes(n)=>format!("BoundedBytes<{n}>"),Type::Fixed(v,n)=>format!("FixedArray<{}, {n}>",rust_type(v)),Type::Variable(v,n)=>format!("BoundedVec<{}, {n}>",rust_type(v)),Type::Option(v)=>format!("Option<{}>",rust_type(v)),Type::Union(v)=>format!("Union{}",v.len()),Type::EvmHead=>"EvmHead".into()}}
fn ts_type(t:&Type)->String{match t{Type::U8|Type::U16|Type::U32|Type::I8|Type::I16|Type::I32=>"number".into(),Type::U64|Type::U128|Type::I64|Type::I128=>"bigint".into(),Type::U256=>"Uint8Array".into(),Type::Bytes(n)=>format!("BoundedBytes<{n}>"),Type::Fixed(v,n)=>format!("FixedArray<{}, {n}>",ts_type(v)),Type::Variable(v,n)=>format!("VariableArray<{}, {n}>",ts_type(v)),Type::Option(v)=>format!("{} | null",ts_type(v)),Type::Union(v)=>v.iter().map(|x|format!("{{tag:{};value:{}}}",x.tag,ts_type(&x.value))).collect::<Vec<_>>().join(" | "),Type::EvmHead=>"EvmHead".into()}}
fn emit_rust_entry(out:&mut String,e:&Entry,entries:&[Entry]){
    let n=entry_ident(e,entries,false);let mut definitions=String::new();
    let input=rust_named_type(&e.input,"Input",&mut definitions);let output=rust_named_type(&e.output,"Output",&mut definitions);
    let mut failure_types=Vec::new();for f in &e.failures{failure_types.push(rust_named_type(&f.detail,&format!("{}Detail",failure_ident(f,&e.failures)),&mut definitions));}
    let _=writeln!(out,"pub mod {n} {{ use super::*;{definitions} pub type Input={input}; pub type Output={output};");
    if e.failures.is_empty(){out.push_str("pub type Failure=core::convert::Infallible;\n");}else{out.push_str("#[derive(Clone,Debug,Eq,PartialEq)]pub enum Failure{");for (f,t) in e.failures.iter().zip(failure_types){let _=write!(out,"{}({t}),",failure_ident(f,&e.failures));}out.push_str("}\n");}
    let convention=if matches!(e.input,Type::EvmHead){2}else{1};let _=write!(out,"pub fn call(input:&Input,deployed_code_hash:[u8;32],published_digest:[u8;32])->Result<Call<Output,Failure>,BindingRefusal>{{check_target(deployed_code_hash,published_digest)?;let mut bytes=vec!{:?};bytes.push({convention});CanonicalEncode::encode(input,&mut bytes).map_err(BindingRefusal::Encode)?;if bytes.len()>MAX_CALLDATA_BYTES+4{{return Err(BindingRefusal::Encode(CodecError::TooLong))}}Ok(Call{{bytes,_type:core::marker::PhantomData}})}}\n",e.discriminator);
    let output_convention=if matches!(e.output,Type::EvmHead){2}else{1};let _=writeln!(out,"pub fn decode_output(bytes:&[u8])->Result<Output,BindingRefusal>{{decode_message(bytes,{output_convention}).map_err(BindingRefusal::Decode)}}");
    out.push_str("pub fn decode_failure(code:u32,bytes:&[u8])->Result<Failure,BindingRefusal>{match code{");for f in &e.failures{let convention=if matches!(f.detail,Type::EvmHead){2}else{1};let _=write!(out,"{}=>decode_message(bytes,{convention}).map(Failure::{}).map_err(BindingRefusal::Decode),",f.code,failure_ident(f,&e.failures));}out.push_str("_=>{let _=bytes;Err(BindingRefusal::UnknownFailure(code))}}}\n}\n");
}

fn rust_named_type(t:&Type,name:&str,definitions:&mut String)->String{match t{
    Type::Fixed(value,n)=>format!("FixedArray<{}, {n}>",rust_named_type(value,&format!("{name}Element"),definitions)),
    Type::Variable(value,n)=>format!("BoundedVec<{}, {n}>",rust_named_type(value,&format!("{name}Element"),definitions)),
    Type::Option(value)=>format!("Option<{}>",rust_named_type(value,&format!("{name}Some"),definitions)),
    Type::Union(variants)=>{let mut shaped=Vec::new();for (index,variant) in variants.iter().enumerate(){shaped.push((variant.tag,rust_named_type(&variant.value,&format!("{name}Variant{index}"),definitions)));}
        let _=write!(definitions,"#[derive(Clone,Debug,Eq,PartialEq)]pub enum {name}{{");for (index,(_,ty)) in shaped.iter().enumerate(){let _=write!(definitions,"Variant{index}({ty}),");}definitions.push_str("}\n");
        let _=write!(definitions,"impl CanonicalEncode for {name}{{fn encode(&self,o:&mut Vec<u8>)->Result<(),CodecError>{{o.push(0x50);match self{{");for (index,(tag,_)) in shaped.iter().enumerate(){let _=write!(definitions,"Self::Variant{index}(v)=>{{o.extend_from_slice(&{tag}u32.to_be_bytes());v.encode(o)?}},");}definitions.push_str("}Ok(())}}\n");
        let _=write!(definitions,"impl CanonicalDecode for {name}{{fn decode(d:&mut Decoder<'_>)->Result<Self,CodecError>{{if d.byte()? !=0x50{{return Err(CodecError::WrongTag)}}match d.u32()?{{");for (index,(tag,ty)) in shaped.iter().enumerate(){let _=write!(definitions,"{tag}=>Ok(Self::Variant{index}(<{ty} as CanonicalDecode>::decode(d)?)),");}definitions.push_str("_=>Err(CodecError::Malformed)}}}\n");name.into()}
    _=>rust_type(t),
}}

fn emit_rust_codec_prelude(out:&mut String){out.push_str(r#"extern crate alloc;
use alloc::vec;
use alloc::vec::Vec;
use core::convert::{TryFrom,TryInto};
pub const MAX_CALLDATA_BYTES:usize=1_048_576;pub const DECODED_SIZE_LIMIT:usize=16_777_216;
#[derive(Clone,Copy,Debug,Eq,PartialEq)]pub enum CodecError{TooLong,WrongLength,WrongConvention,WrongTag,Malformed,Trailing}
#[derive(Clone,Debug,Eq,PartialEq)]pub struct BoundedBytes<const N:usize>(Vec<u8>);
impl<const N:usize> BoundedBytes<N>{pub fn new(value:Vec<u8>)->Result<Self,CodecError>{if value.len()>N{Err(CodecError::TooLong)}else{Ok(Self(value))}}pub fn as_slice(&self)->&[u8]{&self.0}}
#[derive(Clone,Debug,Eq,PartialEq)]pub struct BoundedVec<T,const N:usize>(Vec<T>);
impl<T,const N:usize> BoundedVec<T,N>{pub fn new(value:Vec<T>)->Result<Self,CodecError>{if value.len()>N{Err(CodecError::TooLong)}else{Ok(Self(value))}}pub fn as_slice(&self)->&[T]{&self.0}}
#[derive(Clone,Debug,Eq,PartialEq)]pub struct FixedArray<T,const N:usize>(Vec<T>);
impl<T,const N:usize> FixedArray<T,N>{pub fn new(value:Vec<T>)->Result<Self,CodecError>{if value.len()!=N{Err(CodecError::WrongLength)}else{Ok(Self(value))}}pub fn as_slice(&self)->&[T]{&self.0}}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]pub struct U256(pub [u8;32]);
#[derive(Clone,Debug,Eq,PartialEq)]pub struct EvmHead(BoundedBytes<{MAX_CALLDATA_BYTES-1}>);
impl EvmHead{pub fn new(value:Vec<u8>)->Result<Self,CodecError>{if value.len()%32!=0{return Err(CodecError::WrongLength)}Ok(Self(BoundedBytes::new(value)?))}pub fn as_slice(&self)->&[u8]{self.0.as_slice()}}
pub trait CanonicalEncode{fn encode(&self,out:&mut Vec<u8>)->Result<(),CodecError>;}
pub trait CanonicalDecode:Sized{fn decode(decoder:&mut Decoder<'_>)->Result<Self,CodecError>;}
pub struct Decoder<'a>{bytes:&'a[u8],position:usize,decoded:usize}
impl<'a> Decoder<'a>{fn remaining(&self)->usize{self.bytes.len()-self.position}fn take(&mut self,n:usize)->Result<&'a[u8],CodecError>{self.decoded=self.decoded.checked_add(n).ok_or(CodecError::TooLong)?;if self.decoded>DECODED_SIZE_LIMIT{return Err(CodecError::TooLong)}let end=self.position.checked_add(n).ok_or(CodecError::Malformed)?;let value=self.bytes.get(self.position..end).ok_or(CodecError::Malformed)?;self.position=end;Ok(value)}fn byte(&mut self)->Result<u8,CodecError>{Ok(self.take(1)?[0])}fn u32(&mut self)->Result<u32,CodecError>{Ok(u32::from_be_bytes(self.take(4)?.try_into().map_err(|_|CodecError::Malformed)?))}}
fn decode_message<T:CanonicalDecode>(bytes:&[u8],convention:u8)->Result<T,CodecError>{if bytes.len()>MAX_CALLDATA_BYTES{return Err(CodecError::TooLong)}let (&actual,payload)=bytes.split_first().ok_or(CodecError::Malformed)?;if actual!=convention{return Err(CodecError::WrongConvention)}if convention==2{let value=EvmHead::new(payload.to_vec())?;let mut encoded=Vec::new();value.encode(&mut encoded)?;let mut decoder=Decoder{bytes:&encoded,position:0,decoded:0};let result=T::decode(&mut decoder)?;if decoder.position!=decoder.bytes.len(){return Err(CodecError::Trailing)}return Ok(result)}let mut decoder=Decoder{bytes:payload,position:0,decoded:0};let result=T::decode(&mut decoder)?;if decoder.position!=payload.len(){return Err(CodecError::Trailing)}Ok(result)}
macro_rules! integer_codec{($t:ty,$tag:expr)=>{impl CanonicalEncode for $t{fn encode(&self,o:&mut Vec<u8>)->Result<(),CodecError>{o.push($tag);o.extend_from_slice(&self.to_be_bytes());Ok(())}}impl CanonicalDecode for $t{fn decode(d:&mut Decoder<'_>)->Result<Self,CodecError>{if d.byte()? != $tag{return Err(CodecError::WrongTag)}Ok(<$t>::from_be_bytes(d.take(core::mem::size_of::<$t>())?.try_into().map_err(|_|CodecError::Malformed)?))}}};}
integer_codec!(u8,0x10);integer_codec!(u16,0x11);integer_codec!(u32,0x12);integer_codec!(u64,0x13);integer_codec!(u128,0x14);integer_codec!(i8,0x18);integer_codec!(i16,0x19);integer_codec!(i32,0x1a);integer_codec!(i64,0x1b);integer_codec!(i128,0x1c);
impl CanonicalEncode for U256{fn encode(&self,o:&mut Vec<u8>)->Result<(),CodecError>{o.push(0x15);o.extend_from_slice(&self.0);Ok(())}}impl CanonicalDecode for U256{fn decode(d:&mut Decoder<'_>)->Result<Self,CodecError>{if d.byte()? !=0x15{return Err(CodecError::WrongTag)}Ok(Self(d.take(32)?.try_into().map_err(|_|CodecError::Malformed)?))}}
impl<const N:usize> CanonicalEncode for BoundedBytes<N>{fn encode(&self,o:&mut Vec<u8>)->Result<(),CodecError>{o.push(0x20);o.extend_from_slice(&u32::try_from(self.0.len()).map_err(|_|CodecError::TooLong)?.to_be_bytes());o.extend_from_slice(&self.0);Ok(())}}impl<const N:usize> CanonicalDecode for BoundedBytes<N>{fn decode(d:&mut Decoder<'_>)->Result<Self,CodecError>{if d.byte()? !=0x20{return Err(CodecError::WrongTag)}let n=usize::try_from(d.u32()?).map_err(|_|CodecError::TooLong)?;Self::new(d.take(n)?.to_vec())}}
impl<T:CanonicalEncode,const N:usize> CanonicalEncode for BoundedVec<T,N>{fn encode(&self,o:&mut Vec<u8>)->Result<(),CodecError>{o.push(0x31);o.extend_from_slice(&u32::try_from(self.0.len()).map_err(|_|CodecError::TooLong)?.to_be_bytes());for v in &self.0{v.encode(o)?}Ok(())}}impl<T:CanonicalDecode,const N:usize> CanonicalDecode for BoundedVec<T,N>{fn decode(d:&mut Decoder<'_>)->Result<Self,CodecError>{if d.byte()? !=0x31{return Err(CodecError::WrongTag)}let n=usize::try_from(d.u32()?).map_err(|_|CodecError::TooLong)?;if n>N||n>d.remaining(){return Err(CodecError::TooLong)}let mut v=Vec::with_capacity(n);for _ in 0..n{v.push(T::decode(d)?)}Self::new(v)}}
impl<T:CanonicalEncode,const N:usize> CanonicalEncode for FixedArray<T,N>{fn encode(&self,o:&mut Vec<u8>)->Result<(),CodecError>{o.push(0x30);o.extend_from_slice(&(N as u32).to_be_bytes());for v in &self.0{v.encode(o)?}Ok(())}}impl<T:CanonicalDecode,const N:usize> CanonicalDecode for FixedArray<T,N>{fn decode(d:&mut Decoder<'_>)->Result<Self,CodecError>{if d.byte()? !=0x30||usize::try_from(d.u32()?).map_err(|_|CodecError::WrongLength)?!=N||N>d.remaining(){return Err(CodecError::WrongLength)}let mut v=Vec::with_capacity(N);for _ in 0..N{v.push(T::decode(d)?)}Self::new(v)}}
impl<T:CanonicalEncode> CanonicalEncode for Option<T>{fn encode(&self,o:&mut Vec<u8>)->Result<(),CodecError>{o.push(0x40);match self{None=>o.push(0),Some(v)=>{o.push(1);v.encode(o)?}}Ok(())}}impl<T:CanonicalDecode> CanonicalDecode for Option<T>{fn decode(d:&mut Decoder<'_>)->Result<Self,CodecError>{if d.byte()? !=0x40{return Err(CodecError::WrongTag)}match d.byte()?{0=>Ok(None),1=>Ok(Some(T::decode(d)?)),_=>Err(CodecError::Malformed)}}}
impl CanonicalEncode for EvmHead{fn encode(&self,o:&mut Vec<u8>)->Result<(),CodecError>{o.extend_from_slice(self.as_slice());Ok(())}}impl CanonicalDecode for EvmHead{fn decode(d:&mut Decoder<'_>)->Result<Self,CodecError>{let value=Self::new(d.bytes[d.position..].to_vec())?;d.position=d.bytes.len();Ok(value)}}
"#);}
fn ts_encode(t:&Type,value:&str)->String{match t{
    Type::U8=>format!("unsigned(0x10,1,{value})"),Type::U16=>format!("unsigned(0x11,2,{value})"),Type::U32=>format!("unsigned(0x12,4,{value})"),Type::U64=>format!("unsigned(0x13,8,{value})"),Type::U128=>format!("unsigned(0x14,16,{value})"),
    Type::U256=>format!("concat(Uint8Array.of(0x15),exactBytes({value},32,'u256'))"),Type::I8=>format!("signed(0x18,1,{value})"),Type::I16=>format!("signed(0x19,2,{value})"),Type::I32=>format!("signed(0x1a,4,{value})"),Type::I64=>format!("signed(0x1b,8,{value})"),Type::I128=>format!("signed(0x1c,16,{value})"),
    Type::Bytes(n)=>format!("(()=>{{const v=boundedBytes({n},{value});return concat(Uint8Array.of(0x20),u32(v.length),v);}})()"),
    Type::Fixed(v,n)=>format!("(()=>{{const v=fixedArray({n},{value});return concat(Uint8Array.of(0x30),u32(v.length),...v.map(item=>{}));}})()",ts_encode(v,"item")),
    Type::Variable(v,n)=>format!("(()=>{{const v=variableArray({n},{value});return concat(Uint8Array.of(0x31),u32(v.length),...v.map(item=>{}));}})()",ts_encode(v,"item")),
    Type::Option(v)=>format!("{value}===null?Uint8Array.of(0x40,0):concat(Uint8Array.of(0x40,1),{})",ts_encode(v,value)),
    Type::Union(v)=>{let arms=v.iter().map(|x|format!("case {}:return concat(Uint8Array.of(0x50),u32({}),{});",x.tag,x.tag,ts_encode(&x.value,"value.value"))).collect::<Vec<_>>().join("");format!("((value:any)=>{{switch(value.tag){{{arms}default:return refuse('INVALID_VALUE','unknown union tag');}}}})({value})")},
    Type::EvmHead=>format!("evmHead({value})"),
}}
fn ts_decode(t:&Type,reader:&str)->String{match t{
    Type::U8=>format!("readUnsigned({reader},0x10,1,true) as number"),Type::U16=>format!("readUnsigned({reader},0x11,2,true) as number"),Type::U32=>format!("readUnsigned({reader},0x12,4,true) as number"),Type::U64=>format!("readUnsigned({reader},0x13,8,false) as bigint"),Type::U128=>format!("readUnsigned({reader},0x14,16,false) as bigint"),
    Type::U256=>format!("(()=>{{tag({reader},0x15);return {reader}.take(32);}})()"),Type::I8=>format!("readSigned({reader},0x18,1,true) as number"),Type::I16=>format!("readSigned({reader},0x19,2,true) as number"),Type::I32=>format!("readSigned({reader},0x1a,4,true) as number"),Type::I64=>format!("readSigned({reader},0x1b,8,false) as bigint"),Type::I128=>format!("readSigned({reader},0x1c,16,false) as bigint"),
    Type::Bytes(n)=>format!("(()=>{{tag({reader},0x20);const n={reader}.u32();checkedLength(n,{n},'byte string');return boundedBytes({n},{reader}.take(n));}})()"),
    Type::Fixed(v,n)=>format!("(()=>{{tag({reader},0x30);const n={reader}.u32();if(n!=={n})refuse('NON_CANONICAL','fixed array count does not match schema');const v=[] as Array<{}>;for(let i=0;i<n;i++)v.push({});return fixedArray({n},v);}})()",ts_type(v),ts_decode(v,reader)),
    Type::Variable(v,n)=>format!("(()=>{{tag({reader},0x31);const n={reader}.u32();checkedLength(n,{n},'variable array');const v=[] as Array<{}>;for(let i=0;i<n;i++)v.push({});return variableArray({n},v);}})()",ts_type(v),ts_decode(v,reader)),
    Type::Option(v)=>format!("(()=>{{tag({reader},0x40);const present={reader}.byte();if(present===0)return null;if(present!==1)refuse('NON_CANONICAL','invalid option discriminator');return {};}})()",ts_decode(v,reader)),
    Type::Union(v)=>{let arms=v.iter().map(|x|format!("case {}:return {{tag:{},value:{}}};",x.tag,x.tag,ts_decode(&x.value,reader))).collect::<Vec<_>>().join("");format!("(()=>{{tag({reader},0x50);const variant={reader}.u32();switch(variant){{{arms}default:return refuse('NON_CANONICAL','unknown union tag');}}}})()")},
    Type::EvmHead=>format!("(()=>{{const value={reader}.rest();if(value.length%32!==0)refuse('NON_CANONICAL','EVM head length is not a multiple of 32 bytes');return evmHead(value);}})()"),
}}
fn emit_ts_entry(out:&mut String,e:&Entry,entries:&[Entry]){let n=entry_ident(e,entries,true);let _=writeln!(out,"export type {n}Input = {};\nexport type {n}Output = {};",ts_type(&e.input),ts_type(&e.output));out.push_str(&format!("export type {n}Failure = "));if e.failures.is_empty(){out.push_str("never;\n")}else{for (i,f) in e.failures.iter().enumerate(){if i>0{out.push_str(" | ")}let _=write!(out,"{{ code: {}; name: '{}'; detail: {} }}",f.code,f.name,ts_type(&f.detail));}out.push_str(";\n")}
    let input_convention=if matches!(e.input,Type::EvmHead){2}else{1};let output_convention=if matches!(e.output,Type::EvmHead){2}else{1};
    let _=writeln!(out,"export function encode{n}(input:{n}Input,deployedCodeHash:string,publishedDigest:string):LayerXCall<{n}Output,{n}Failure>{{checkTarget(deployedCodeHash,publishedDigest);return layerXCall<{n}Output,{n}Failure>(concat(Uint8Array.from({:?}),frame({input_convention},{})));}}",e.discriminator,ts_encode(&e.input,"input"));
    let _=writeln!(out,"export function decode{n}Output(bytes:Uint8Array):{n}Output{{const reader=readerFor(bytes,{output_convention});return finish(reader,{});}}",ts_decode(&e.output,"reader"));
    if !e.failures.is_empty(){let arms=e.failures.iter().map(|f|{let convention=if matches!(f.detail,Type::EvmHead){2}else{1};format!("case {}:{{const reader=readerFor(detail,{});return {{code:{},name:'{}',detail:finish(reader,{})}};}}",f.code,convention,f.code,f.name,ts_decode(&f.detail,"reader"))}).collect::<Vec<_>>().join("");let _=writeln!(out,"export function decode{n}Failure(code:number,detail:Uint8Array):{n}Failure{{switch(code){{{arms}default:return refuse('UNKNOWN_FAILURE',`unknown {n} failure code ${{code}}`);}}}}");}
}
fn emit_guest_entry(out:&mut String,e:&Entry,entries:&[Entry]){
    let n=entry_ident(e,entries,false);let mut definitions=String::new();
    let input=rust_named_type(&e.input,"Input",&mut definitions);let output=rust_named_type(&e.output,"Output",&mut definitions);
    let mut failure_types=Vec::new();for f in &e.failures{failure_types.push(rust_named_type(&f.detail,&format!("{}Detail",failure_ident(f,&e.failures)),&mut definitions));}
    let _=writeln!(out,"pub mod {n}{{use super::*;{definitions}pub type Input={input};pub type Output={output};");
    if e.failures.is_empty(){out.push_str("pub type Failure=core::convert::Infallible;\nfn encode_failure(value:Failure)->Result<(u32,Vec<u8>),CodecError>{match value{}}\n");}else{out.push_str("#[derive(Clone,Debug,Eq,PartialEq)]pub enum Failure{");for (f,t) in e.failures.iter().zip(&failure_types){let _=write!(out,"{}({t}),",failure_ident(f,&e.failures));}out.push_str("}\nfn encode_failure(value:Failure)->Result<(u32,Vec<u8>),CodecError>{match value{");for f in &e.failures{let convention=if matches!(f.detail,Type::EvmHead){2}else{1};let _=write!(out,"Failure::{}(v)=>{{let mut detail=vec![{convention}];v.encode(&mut detail)?;if detail.len()>MAX_CALLDATA_BYTES{{return Err(CodecError::TooLong)}}Ok(({},detail))}},",failure_ident(f,&e.failures),f.code);}out.push_str("}}\n");}
    let input_convention=if matches!(e.input,Type::EvmHead){2}else{1};let output_convention=if matches!(e.output,Type::EvmHead){2}else{1};
    let _=write!(out,"pub(super) fn dispatch<P:Program>(p:&mut P,b:&[u8])->Result<Vec<u8>,DispatchFailure>{{let input=decode_message::<Input>(b,{input_convention}).map_err(DispatchFailure::Decode)?;match p.{n}(input){{Ok(v)=>{{let mut o=vec![{output_convention}];v.encode(&mut o).map_err(DispatchFailure::Encode)?;if o.len()>MAX_CALLDATA_BYTES{{return Err(DispatchFailure::Encode(CodecError::TooLong))}}Ok(o)}},Err(e)=>{{let(code,detail)=encode_failure(e).map_err(DispatchFailure::Encode)?;Err(DispatchFailure::Typed{{code,detail}})}}}}}}}}\n");
}
fn frozen_vectors()->[(&'static str,&'static [u8]);19]{[
    ("u8",&[0x10,0x7f]),("u16",&[0x11,0x12,0x34]),("u32",&[0x12,0,0,0,7]),("u64",&[0x13,0,0,0,0,0,0,0,9]),
    ("u128",&[0x14,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,11]),("u256",&[0x15,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,13]),
    ("i8",&[0x18,0xff]),("i16",&[0x19,0xff,0xfe]),("i32",&[0x1a,0xff,0xff,0xff,0xfd]),("i64",&[0x1b,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xfc]),
    ("i128",&[0x1c,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xfb]),
    ("bytes",&[0x20,0,0,0,3,1,2,3]),("fixed",&[0x30,0,0,0,2,0x10,1,0x10,2]),("variable",&[0x31,0,0,0,2,0x11,0,1,0x11,0,2]),
    ("none",&[0x40,0]),("some",&[0x40,1,0x12,0,0,0,8]),("union0",&[0x50,0,0,0,0,0x10,9]),("union7",&[0x50,0,0,0,7,0x11,0,10]),
    ("evm",&[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,1]),
]}
fn emit_rust_frozen_vectors(out:&mut String){out.push_str("pub const FROZEN_CODEC_VECTORS:&[(&str,&[u8])]=&[");for(name,bytes)in frozen_vectors(){let _=write!(out,"(\"{name}\",&{:?}),",bytes);}out.push_str("];\n");}
fn emit_ts_frozen_vectors(out:&mut String){out.push_str("export const FROZEN_CODEC_VECTORS = [");for(name,bytes)in frozen_vectors(){let _=write!(out,"['{name}',Uint8Array.from({:?})],",bytes);}out.push_str("] as const;\n");}
fn rust_ident(s:&str)->String{let mut o=s.to_ascii_lowercase();if o.as_bytes().first().is_some_and(u8::is_ascii_digit){o.insert_str(0,"n_")}if rust_keyword(&o){o.push_str("_binding")}o}
fn pascal(s:&str)->String{let mut o=String::new();let mut upper=true;for c in s.chars(){if c=='_'{upper=true}else if upper{o.extend(c.to_uppercase());upper=false}else{o.push(c)}}if o.is_empty(){o.push_str("Name")}if o.as_bytes()[0].is_ascii_digit(){o.insert(0,'N')}o}
fn rust_keyword(s:&str)->bool{matches!(s,"as"|"break"|"const"|"continue"|"crate"|"else"|"enum"|"extern"|"false"|"fn"|"for"|"if"|"impl"|"in"|"let"|"loop"|"match"|"mod"|"move"|"mut"|"pub"|"ref"|"return"|"self"|"static"|"struct"|"super"|"trait"|"true"|"type"|"unsafe"|"use"|"where"|"while"|"async"|"await"|"dyn"|"abstract"|"become"|"box"|"do"|"final"|"macro"|"override"|"priv"|"typeof"|"unsized"|"virtual"|"yield"|"try"|"gen"|"union"|"macro_rules"|"raw")}
fn entry_ident(entry:&Entry,entries:&[Entry],typescript:bool)->String{let base_of=|item:&Entry|if typescript{pascal(&item.name)}else{rust_ident(&item.name)};let reserved:BTreeSet<String>=entries.iter().map(&base_of).collect();let mut used=BTreeSet::new();for item in entries{let base=base_of(item);let mut candidate=if entries.iter().filter(|other|base_of(other)==base).count()==1{base.clone()}else if typescript{format!("{base}Lx{}",hex(&item.discriminator))}else{format!("{base}_lx_{}",hex(&item.discriminator))};while used.contains(&candidate)||(candidate!=base&&reserved.contains(&candidate)){candidate.push('_')}if item.discriminator==entry.discriminator{return candidate}used.insert(candidate)}unreachable!()}
fn failure_ident(failure:&Failure,failures:&[Failure])->String{let reserved: BTreeSet<String>=failures.iter().map(|item|pascal(&item.name)).collect();let mut used=BTreeSet::new();for item in failures{let base=pascal(&item.name);let mut candidate=if failures.iter().filter(|other|pascal(&other.name)==base).count()==1{base.clone()}else{format!("{base}LxCode{}",item.code)};while used.contains(&candidate)||(candidate!=base&&reserved.contains(&candidate)){candidate.push('_')}if item.code==failure.code{return candidate}used.insert(candidate)}unreachable!()}
fn generated_header(language:&str,digest:[u8;32])->String{format!("// Generated {language} binding. Interface SHA-256: {}. Do not edit.\n",hex(&digest))}
fn hex(bytes:&[u8])->String{let mut out=String::new();for b in bytes{let _=write!(out,"{b:02x}");}out}
fn hex_array(out:&mut String,bytes:&[u8]){for b in bytes{let _=write!(out,"0x{b:02x},");}}

#[cfg(test)]
mod vectors {
    use super::*;
    #[test] fn stale_digest_is_a_typed_refusal(){let generator=BindingGenerator{digest:[7;32],code_hash:[9;32],entries:Vec::new()};assert_eq!(generator.require_digest([8;32]),Err(BindgenError::StaleBinding{expected:[7;32],published:[8;32]}));assert_eq!(generator.require_code_hash([1;32]),Err(BindgenError::CodeHashMismatch{expected:[9;32],deployed:[1;32]}));}
    #[test] fn every_schema_type_has_both_language_shapes(){let all=[Type::U8,Type::U16,Type::U32,Type::U64,Type::U128,Type::U256,Type::I8,Type::I16,Type::I32,Type::I64,Type::I128,Type::Bytes(4),Type::Fixed(Box::new(Type::U8),2),Type::Variable(Box::new(Type::U16),3),Type::Option(Box::new(Type::U32)),Type::Union(vec![Variant{tag:0,value:Type::U8}]),Type::EvmHead];for value in all{assert!(!rust_type(&value).is_empty());assert!(!ts_type(&value).is_empty());}}
    #[test] fn rust_and_typescript_publish_the_same_frozen_codec_vectors(){let generator=BindingGenerator{digest:[7;32],code_hash:[9;32],entries:Vec::new()};let rust=generator.generate_rust();let typescript=generator.generate_typescript();for(name,bytes)in frozen_vectors(){assert!(rust.contains(name));assert!(typescript.contains(name));assert!(!bytes.is_empty());}assert_eq!(frozen_vectors()[15].1,[0x40,1,0x12,0,0,0,8]);assert_eq!(frozen_vectors()[18].1.len(),32);}
    #[test] fn generated_calls_refuse_stale_targets_before_encoding(){let entry=Entry{name:"call".into(),discriminator:[1,2,3,4],input:Type::U8,output:Type::U16,failures:vec![Failure{code:7,name:"denied".into(),detail:Type::Bytes(8)}]};let generator=BindingGenerator{digest:[7;32],code_hash:[9;32],entries:vec![entry]};let rust=generator.generate_rust();let typescript=generator.generate_typescript();let rust_check=rust.find("check_target(deployed_code_hash,published_digest)?").unwrap_or(usize::MAX);let rust_encode=rust.find("CanonicalEncode::encode(input").unwrap_or(0);assert!(rust_check<rust_encode);let ts_check=typescript.find("checkTarget(deployedCodeHash,publishedDigest);").unwrap_or(usize::MAX);let ts_encode=typescript.find("unsigned(0x10,1,input)").unwrap_or(0);assert!(ts_check<ts_encode);assert!(rust.contains("Failure::Denied"));assert!(typescript.contains("code: 7; name: 'denied'"));}
}
