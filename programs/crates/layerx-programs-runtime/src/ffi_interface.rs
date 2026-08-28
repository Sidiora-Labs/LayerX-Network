use crate::abi::{EncodingConvention, TypeTag};
use crate::WasmEngine;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const OK: i32 = 0;
const NON_CANONICAL: i32 = -3;
const VERSION_UNSUPPORTED: i32 = -101;
const LENGTH_LIMIT: i32 = -5;
const CONTEXT_MISMATCH: i32 = -213;
const DOMAIN: &[u8] = b"LayerX/program-interface/v1\0";
const MAX_BYTES: usize = 952;
const MAX_ENTRIES: usize = 256;
const MAX_FIELDS: usize = 256;
const MAX_DEPTH: usize = 16;
const MAX_NAME: usize = 128;

unsafe extern "C" {
    fn layerx_programs_interface_activity_byte(token: u64, section: u16, offset: u32) -> i32;
}

#[derive(Clone, Eq, PartialEq)]
struct Schema { convention: EncodingConvention, value: Option<ValueType> }
#[derive(Clone, Eq, PartialEq)]
enum ValueType {
    U8, U16, U32, U64, U128, U256, I8, I16, I32, I64, I128, Bytes(u32),
    FixedArray(u32, Box<Self>), VariableArray(u32, Box<Self>), Option(Box<Self>),
    Union(Vec<Variant>),
}
#[derive(Clone, Eq, PartialEq)]
struct Variant { tag: u32, value: ValueType }
#[derive(Clone, Eq, PartialEq)]
struct Failure { code: u32, name: String, schema: Schema }
#[derive(Clone, Eq, PartialEq)]
struct Entry {
    name: String, discriminator: [u8; 4], calldata: Schema, response: Schema,
    capabilities: Vec<Vec<u8>>, topics: Vec<[u8; 32]>, failures: Vec<Failure>,
}
struct Interface { hash: [u8; 32], abi: u16, entries: Vec<Entry> }
fn capability_mask(capabilities:&[Vec<u8>])->u16{capabilities.iter().fold(0u16,|mask,c|mask|c.first().map_or(0,|tag|1u16<<u32::from(*tag)))}

fn bytes(token: u64, section: u16, length: usize) -> Result<Vec<u8>, i32> {
    if token == 0 || length == 0 || length > MAX_BYTES { return Err(LENGTH_LIMIT); }
    let length = u32::try_from(length).map_err(|_| LENGTH_LIMIT)?;
    let mut out = Vec::with_capacity(length as usize);
    for offset in 0..length {
        let value = unsafe { layerx_programs_interface_activity_byte(token, section, offset) };
        out.push(u8::try_from(value).map_err(|_| if value < 0 { value } else { NON_CANONICAL })?);
    }
    Ok(out)
}

fn take<const N: usize>(input: &[u8], cursor: &mut usize) -> Result<[u8; N], i32> {
    let end = cursor.checked_add(N).ok_or(NON_CANONICAL)?;
    let value = input.get(*cursor..end).ok_or(NON_CANONICAL)?.try_into().map_err(|_| NON_CANONICAL)?;
    *cursor = end; Ok(value)
}
fn text(input: &[u8], cursor: &mut usize) -> Result<String, i32> {
    let length = usize::from(u16::from_be_bytes(take::<2>(input, cursor)?));
    let end = cursor.checked_add(length).ok_or(NON_CANONICAL)?;
    let value = core::str::from_utf8(input.get(*cursor..end).ok_or(NON_CANONICAL)?)
        .map_err(|_| NON_CANONICAL)?.to_owned();
    *cursor = end;
    if value.is_empty() || value.len() > MAX_NAME || !value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') { return Err(NON_CANONICAL); }
    Ok(value)
}
fn count(input: &[u8], cursor: &mut usize) -> Result<usize, i32> {
    let value = usize::from(u16::from_be_bytes(take::<2>(input, cursor)?));
    if value > MAX_FIELDS { Err(NON_CANONICAL) } else { Ok(value) }
}
fn schema(input: &[u8], cursor: &mut usize, depth: usize) -> Result<Schema, i32> {
    if depth > MAX_DEPTH { return Err(NON_CANONICAL); }
    let convention=EncodingConvention::from_tag(take::<1>(input,cursor)?[0]).map_err(|_|NON_CANONICAL)?;
    let value=match convention { EncodingConvention::LayerX=>Some(value_type(input,cursor,depth)?), EncodingConvention::EvmHeadOnly=>None };
    Ok(Schema{convention,value})
}
fn value_type(input:&[u8],cursor:&mut usize,depth:usize)->Result<ValueType,i32>{
    if depth>MAX_DEPTH{return Err(NON_CANONICAL)}
    Ok(match TypeTag::from_byte(take::<1>(input,cursor)?[0]).map_err(|_|NON_CANONICAL)? {
        TypeTag::U8=>ValueType::U8,TypeTag::U16=>ValueType::U16,TypeTag::U32=>ValueType::U32,TypeTag::U64=>ValueType::U64,TypeTag::U128=>ValueType::U128,TypeTag::U256=>ValueType::U256,
        TypeTag::I8=>ValueType::I8,TypeTag::I16=>ValueType::I16,TypeTag::I32=>ValueType::I32,TypeTag::I64=>ValueType::I64,TypeTag::I128=>ValueType::I128,
        TypeTag::Bytes=>{let n=u32::from_be_bytes(take::<4>(input,cursor)?);if n==0{return Err(NON_CANONICAL)}ValueType::Bytes(n)},
        TypeTag::FixedArray=>{let n=u32::from_be_bytes(take::<4>(input,cursor)?);if n==0{return Err(NON_CANONICAL)}ValueType::FixedArray(n,Box::new(value_type(input,cursor,depth+1)?))},
        TypeTag::VariableArray=>{let n=u32::from_be_bytes(take::<4>(input,cursor)?);if n==0{return Err(NON_CANONICAL)}ValueType::VariableArray(n,Box::new(value_type(input,cursor,depth+1)?))},
        TypeTag::Option=>ValueType::Option(Box::new(value_type(input,cursor,depth+1)?)),
        TypeTag::Union=>{let n=count(input,cursor)?;if n==0{return Err(NON_CANONICAL)}let mut v=Vec::with_capacity(n);for _ in 0..n{v.push(Variant{tag:u32::from_be_bytes(take::<4>(input,cursor)?),value:value_type(input,cursor,depth+1)?})}if !v.windows(2).all(|p|p[0].tag<p[1].tag){return Err(NON_CANONICAL)}ValueType::Union(v)},
    })
}
fn decode(input: &[u8]) -> Result<Interface, i32> {
    if input.len()>MAX_BYTES || input.get(..DOMAIN.len())!=Some(DOMAIN){return Err(NON_CANONICAL)}
    let mut c=DOMAIN.len(); let hash=take::<32>(input,&mut c)?; let abi=u16::from_be_bytes(take::<2>(input,&mut c)?);
    if hash==[0;32] || !matches!(abi,1|2){return Err(VERSION_UNSUPPORTED)}
    let n=count(input,&mut c)?; if n==0||n>MAX_ENTRIES{return Err(NON_CANONICAL)} let mut entries=Vec::with_capacity(n);
    for _ in 0..n {
        let name=text(input,&mut c)?; let discriminator=take::<4>(input,&mut c)?; let calldata=schema(input,&mut c,0)?; let response=schema(input,&mut c,0)?;
        let cn=count(input,&mut c)?; let mut capabilities=Vec::with_capacity(cn); for _ in 0..cn {capabilities.push(capability(input,&mut c)?)}
        if !capabilities.windows(2).all(|p|p[0]<p[1]){return Err(NON_CANONICAL)}
        let tn=count(input,&mut c)?; let mut topics=Vec::with_capacity(tn); for _ in 0..tn{topics.push(take::<32>(input,&mut c)?)} if !topics.windows(2).all(|p|p[0]<p[1]){return Err(NON_CANONICAL)}
        let en=count(input,&mut c)?; let mut failures=Vec::with_capacity(en); for _ in 0..en{failures.push(Failure{code:u32::from_be_bytes(take::<4>(input,&mut c)?),name:text(input,&mut c)?,schema:schema(input,&mut c,0)?})} if !failures.windows(2).all(|p|p[0].code<p[1].code){return Err(NON_CANONICAL)}
        entries.push(Entry{name,discriminator,calldata,response,capabilities,topics,failures});
    }
    let discriminators: BTreeSet<_> = entries.iter().map(|entry| entry.discriminator).collect();
    if c!=input.len() || !entries.windows(2).all(|p|p[0].name<p[1].name) || discriminators.len()!=entries.len(){return Err(NON_CANONICAL)}
    Ok(Interface{hash,abi,entries})
}
fn capability(input:&[u8],cursor:&mut usize)->Result<Vec<u8>,i32>{let start=*cursor;match take::<1>(input,cursor)?[0]{0..=4=>{},5|8=>{if take::<32>(input,cursor)?==[0;32]{return Err(NON_CANONICAL)}},6=>{let asset=take::<32>(input,cursor)?;let to=take::<32>(input,cursor)?;if asset==[0;32]||to==[0;32]||u128::from_be_bytes(take::<16>(input,cursor)?)==0{return Err(NON_CANONICAL)}},7=>{if take::<32>(input,cursor)?==[0;32]{return Err(NON_CANONICAL)}let n=usize::from(u16::from_be_bytes(take::<2>(input,cursor)?));if n==0||n>crate::MAX_PROGRAM_ACCOUNT_SEED_BYTES{return Err(NON_CANONICAL)};let end=cursor.checked_add(n).ok_or(NON_CANONICAL)?;if input.get(*cursor..end).is_none(){return Err(NON_CANONICAL)}*cursor=end;let source=take::<32>(input,cursor)?;let asset=take::<32>(input,cursor)?;let to=take::<32>(input,cursor)?;if source==[0;32]||asset==[0;32]||to==[0;32]||u128::from_be_bytes(take::<16>(input,cursor)?)==0{return Err(NON_CANONICAL)}},9=>{let account=take::<32>(input,cursor)?;let asset=take::<32>(input,cursor)?;let receipt=take::<32>(input,cursor)?;if account==[0;32]||asset==[0;32]||receipt==[0;32]{return Err(NON_CANONICAL)}},_=>return Err(NON_CANONICAL)}Ok(input[start..*cursor].to_vec())}
fn accepts(new:&Schema,old:&Schema)->bool{new.convention==old.convention&&match(&new.value,&old.value){(None,None)=>true,(Some(a),Some(b))=>accepts_value(a,b),_=>false}}
fn accepts_value(new:&ValueType,old:&ValueType)->bool{match(new,old){
    (ValueType::U8,ValueType::U8)|(ValueType::U16,ValueType::U16)|(ValueType::U32,ValueType::U32)|(ValueType::U64,ValueType::U64)|(ValueType::U128,ValueType::U128)|(ValueType::U256,ValueType::U256)|(ValueType::I8,ValueType::I8)|(ValueType::I16,ValueType::I16)|(ValueType::I32,ValueType::I32)|(ValueType::I64,ValueType::I64)|(ValueType::I128,ValueType::I128)=>true,
    (ValueType::Bytes(a),ValueType::Bytes(b))=>a>=b,
    (ValueType::FixedArray(an,a),ValueType::FixedArray(bn,b))=>an==bn&&accepts_value(a,b),
    (ValueType::VariableArray(an,a),ValueType::VariableArray(bn,b))=>an>=bn&&accepts_value(a,b),
    (ValueType::Option(a),ValueType::Option(b))=>accepts_value(a,b),
    (ValueType::Union(a),ValueType::Union(b))=>b.iter().all(|old|a.iter().any(|new|new.tag==old.tag&&accepts_value(&new.value,&old.value))),_=>false}}
fn widening(new:&Interface,old:&Interface)->bool {new.abi==old.abi&&old.entries.iter().all(|o|new.entries.iter().find(|n|n.name==o.name).is_some_and(|n|n.discriminator==o.discriminator&&accepts(&n.calldata,&o.calldata)&&accepts(&o.response,&n.response)&&n.capabilities.iter().all(|x|o.capabilities.contains(x))&&o.topics.iter().all(|x|n.topics.contains(x))&&o.failures.iter().all(|x|n.failures.contains(x))))}

#[no_mangle]
pub extern "C" fn layerx_programs_interface_validate(token:u64,wasm_length:u32,interface_length:u32,prior_length:u32,abi:u16,breaking:u8,h0:u64,h1:u64,h2:u64,h3:u64)->i32 {
    let wasm=match bytes(token,0,wasm_length as usize){Ok(x)=>x,Err(e)=>return e}; let encoded=match bytes(token,2,interface_length as usize){Ok(x)=>x,Err(e)=>return e}; let interface=match decode(&encoded){Ok(x)=>x,Err(e)=>return e};
    let mut hash=[0u8;32]; for(c,w)in hash.chunks_exact_mut(8).zip([h0,h1,h2,h3]){c.copy_from_slice(&w.to_be_bytes())}
    if interface.hash!=hash||interface.abi!=abi||<[u8;32]>::from(Sha256::digest(&wasm))!=hash{return CONTEXT_MISMATCH}
    let engine=match WasmEngine::declared(){Ok(x)=>x,Err(_)=>return NON_CANONICAL}; let module=match match abi{1=>engine.validate(&wasm),2=>engine.validate_v2(&wasm),_=>return VERSION_UNSUPPORTED}{Ok(x)=>x,Err(_)=>return NON_CANONICAL};
    if interface.entries.iter().any(|e|!module.supports_interface_entrypoint(&e.name)||!module.interface_capability_mask_matches(&e.name,capability_mask(&e.capabilities))){return NON_CANONICAL}
    if prior_length>0 {let prior=match bytes(token,3,prior_length as usize).and_then(|x|decode(&x)){Ok(x)=>x,Err(e)=>return e}; if breaking==0&&!widening(&interface,&prior){return NON_CANONICAL}}
    OK
}
