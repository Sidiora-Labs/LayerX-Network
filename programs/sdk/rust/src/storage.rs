//! Namespaced storage bindings.
//!
//! Keys address the calling program's own namespace, which the runtime fixes
//! before guest entry. The namespace may be principal-scoped `(program,
//! principal)` or program-shared `(program)`, and the type system makes it
//! impossible to address the wrong scope by accident.

use crate::abi::{MAX_STORAGE_KEY_BYTES, MAX_STORAGE_VALUE_BYTES};
use crate::error::{Field, ProgramError, Reason};

#[cfg(target_arch = "wasm32")]
use crate::buffer::Bytes;
#[cfg(target_arch = "wasm32")]
use crate::host;

/// A key inside this program's own namespace.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StorageKey<'a>(&'a [u8]);

impl<'a> StorageKey<'a> {
    /// Constructs a key inside the version-one storage bound.
    ///
    /// # Errors
    ///
    /// Refuses an empty key and a key past the declared bound.
    pub const fn new(bytes: &'a [u8]) -> Result<Self, ProgramError> {
        if bytes.is_empty() {
            return Err(ProgramError::value(Field::StorageKey, Reason::Empty));
        }
        if bytes.len() > MAX_STORAGE_KEY_BYTES {
            return Err(ProgramError::value(Field::StorageKey, Reason::TooLarge));
        }
        Ok(Self(bytes))
    }

    /// Borrows the canonical key bytes.
    #[must_use]
    pub const fn bytes(self) -> &'a [u8] {
        self.0
    }
}

/// A value stored inside this program's own namespace.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StorageValue<'a>(&'a [u8]);

impl<'a> StorageValue<'a> {
    /// Constructs a value inside the version-one storage bound.
    ///
    /// # Errors
    ///
    /// Refuses a value past the declared bound.
    pub const fn new(bytes: &'a [u8]) -> Result<Self, ProgramError> {
        if bytes.len() > MAX_STORAGE_VALUE_BYTES {
            return Err(ProgramError::value(Field::StorageValue, Reason::TooLarge));
        }
        Ok(Self(bytes))
    }

    /// Borrows the canonical value bytes.
    #[must_use]
    pub const fn bytes(self) -> &'a [u8] {
        self.0
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn decode_read_length(status: i32, capacity: usize) -> Result<Option<usize>, ProgramError> {
    if status == 0 {
        return Ok(None);
    }
    let reported = usize::try_from(status)
        .map_err(|_| ProgramError::value(Field::StorageValue, Reason::Malformed))?;
    let length = reported
        .checked_sub(1)
        .ok_or_else(|| ProgramError::value(Field::StorageValue, Reason::Malformed))?;
    if length > capacity {
        return Err(ProgramError::value(Field::StorageValue, Reason::Malformed));
    }
    Ok(Some(length))
}

/// Reads one value into a caller-owned buffer, returning its length.
///
/// Returns `None` when the key holds no value.
///
/// # Errors
///
/// Refuses missing read authority, a buffer shorter than the stored value,
/// and every meter refusal.
#[cfg(target_arch = "wasm32")]
pub fn read(key: StorageKey<'_>, output: &mut [u8]) -> Result<Option<usize>, ProgramError> {
    let status = host::storage_read(key.bytes(), output)?;
    decode_read_length(status, output.len())
}

/// Reads one value into a fixed-capacity buffer, reporting whether the key
/// held a value at all.
///
/// # Errors
///
/// Refuses missing read authority, a buffer shorter than the stored value,
/// and every meter refusal.
#[cfg(target_arch = "wasm32")]
pub fn read_into<const N: usize>(
    key: StorageKey<'_>,
    output: &mut Bytes<N>,
) -> Result<bool, ProgramError> {
    output.clear();
    let Some(length) = read(key, output.as_mut_slice())? else {
        return Ok(false);
    };
    output.set_length(length)?;
    Ok(true)
}

/// Stages one value in this program's namespace.
///
/// # Errors
///
/// Refuses missing write authority, invalid bounds, and every meter refusal.
#[cfg(target_arch = "wasm32")]
pub fn write(key: StorageKey<'_>, value: StorageValue<'_>) -> Result<(), ProgramError> {
    host::storage_write(key.bytes(), value.bytes())?;
    Ok(())
}

/// Stages the deletion of one key in this program's namespace.
///
/// # Errors
///
/// Refuses missing write authority, invalid keys, and every meter refusal.
#[cfg(target_arch = "wasm32")]
pub fn delete(key: StorageKey<'_>) -> Result<(), ProgramError> {
    host::storage_delete(key.bytes())?;
    Ok(())
}

/// Maximum portable continuation cursor width frozen by ABI v2.
pub const MAX_SCAN_CURSOR_BYTES: usize = 591;
/// Maximum entries returned by one scan.
pub const MAX_SCAN_ENTRIES: u32 = 64;

/// Opaque portable continuation issued by a prior scan.
pub struct ScanCursor<'a>(&'a [u8]);
impl<'a> ScanCursor<'a>{
    /// Constructs an opaque bounded cursor. # Errors Refuses an oversized cursor.
    pub const fn new(bytes:&'a [u8])->Result<Self,ProgramError>{if bytes.len()>MAX_SCAN_CURSOR_BYTES{Err(ProgramError::value(Field::Buffer,Reason::TooLarge))}else{Ok(Self(bytes))}}
    /// Starts a scan without a continuation.
    #[must_use] pub const fn start()->Self{Self(&[])}
    /// Borrows the canonical cursor bytes.
    #[must_use] pub const fn bytes(&self)->&'a [u8]{self.0}
}

/// Explicit bounded page contract for a storage scan.
#[derive(Clone,Copy)]
pub struct ScanLimits{max_entries:u32,max_bytes:u32}
impl ScanLimits{
    /// Constructs scan limits. # Errors Refuses zero or protocol-widening limits.
    pub const fn new(max_entries:u32,max_bytes:u32)->Result<Self,ProgramError>{if max_entries==0||max_entries>MAX_SCAN_ENTRIES||max_bytes<5||max_bytes>67_126_228{Err(ProgramError::value(Field::Buffer,Reason::TooLarge))}else{Ok(Self{max_entries,max_bytes})}}
}

/// One borrowed key/value entry from a canonical scan page.
pub struct ScanEntry<'a>{key:&'a [u8],value:&'a [u8]}
impl<'a> ScanEntry<'a>{#[must_use] pub const fn key(&self)->&'a [u8]{self.key} #[must_use] pub const fn value(&self)->&'a [u8]{self.value}}

/// Canonically decoded scan page borrowing the caller-owned output buffer.
pub struct ScanPage<'a>{encoded:&'a [u8],entries_end:usize,cursor_start:usize,cursor_end:usize}
impl<'a> ScanPage<'a>{
    fn decode(encoded:&'a [u8],prefix:&[u8],limits:ScanLimits)->Result<Self,ProgramError>{
        let malformed=||ProgramError::value(Field::Buffer,Reason::Malformed);
        if encoded.len()>usize::try_from(limits.max_bytes).map_err(|_|malformed())?{return Err(malformed());}
        let mut decoder=PageDecoder::new(encoded);let count=decoder.u16()?;if u32::from(count)>limits.max_entries||u32::from(count)>MAX_SCAN_ENTRIES{return Err(malformed());}
        let mut previous:Option<&[u8]>=None;
        for _ in 0..count{let key_len=usize::from(decoder.u16()?);if key_len==0||key_len>MAX_STORAGE_KEY_BYTES{return Err(malformed());}let key=decoder.take(key_len)?;if !key.starts_with(prefix)||previous.is_some_and(|prior|prior>=key){return Err(malformed());}previous=Some(key);let value_len=usize::try_from(decoder.u32()?).map_err(|_|malformed())?;if value_len>MAX_STORAGE_VALUE_BYTES{return Err(malformed());}decoder.take(value_len)?;}
        let entries_end=decoder.offset;let present=decoder.byte()?;if present>1{return Err(malformed());}let cursor_len=usize::from(decoder.u16()?);if cursor_len>MAX_SCAN_CURSOR_BYTES{return Err(malformed());}let cursor_start=decoder.offset;decoder.take(cursor_len)?;let cursor_end=decoder.offset;if !decoder.is_empty()||(present==0&&cursor_len!=0)||(present==1&&cursor_len==0){return Err(malformed());}Ok(Self{encoded,entries_end,cursor_start,cursor_end})
    }
    /// Iterates entries in canonical key order.
    pub fn entries(&self)->ScanEntries<'a>{ScanEntries{remaining:&self.encoded[2..self.entries_end]}}
    /// Returns the continuation when more entries remain.
    pub fn cursor(&self)->Option<ScanCursor<'a>>{if self.cursor_start==self.cursor_end{None}else{Some(ScanCursor(&self.encoded[self.cursor_start..self.cursor_end]))}}
}

struct PageDecoder<'a>{bytes:&'a [u8],offset:usize}
impl<'a> PageDecoder<'a>{fn new(bytes:&'a [u8])->Self{Self{bytes,offset:0}}fn take(&mut self,length:usize)->Result<&'a [u8],ProgramError>{let end=self.offset.checked_add(length).ok_or_else(||ProgramError::value(Field::Buffer,Reason::Malformed))?;let value=self.bytes.get(self.offset..end).ok_or_else(||ProgramError::value(Field::Buffer,Reason::Malformed))?;self.offset=end;Ok(value)}fn byte(&mut self)->Result<u8,ProgramError>{Ok(self.take(1)?[0])}fn u16(&mut self)->Result<u16,ProgramError>{Ok(u16::from_be_bytes(self.take(2)?.try_into().map_err(|_|ProgramError::value(Field::Buffer,Reason::Malformed))?))}fn u32(&mut self)->Result<u32,ProgramError>{Ok(u32::from_be_bytes(self.take(4)?.try_into().map_err(|_|ProgramError::value(Field::Buffer,Reason::Malformed))?))}fn is_empty(&self)->bool{self.offset==self.bytes.len()}}

/// Iterator over one validated canonical page.
pub struct ScanEntries<'a>{remaining:&'a [u8]}
impl<'a> Iterator for ScanEntries<'a>{type Item=ScanEntry<'a>;fn next(&mut self)->Option<Self::Item>{if self.remaining.is_empty(){return None;}let key_len=usize::from(u16::from_be_bytes(self.remaining[..2].try_into().ok()?));let key=&self.remaining[2..2+key_len];let value_offset=2+key_len;let value_len=usize::try_from(u32::from_be_bytes(self.remaining[value_offset..value_offset+4].try_into().ok()?)).ok()?;let value=&self.remaining[value_offset+4..value_offset+4+value_len];self.remaining=&self.remaining[value_offset+4+value_len..];Some(ScanEntry{key,value})}}

#[cfg(target_arch="wasm32")]
fn scan_with<'a>(host_call:fn(&[u8],&[u8],u32,u32,&mut[u8])->Result<usize,ProgramError>,prefix:&[u8],cursor:&ScanCursor<'_>,limits:ScanLimits,output:&'a mut[u8])->Result<ScanPage<'a>,ProgramError>{if prefix.len()>MAX_STORAGE_KEY_BYTES||usize::try_from(limits.max_bytes).map_err(|_|ProgramError::value(Field::Buffer,Reason::TooLarge))?>output.len(){return Err(ProgramError::value(Field::Buffer,Reason::TooLarge));}let length=host_call(prefix,cursor.bytes(),limits.max_entries,limits.max_bytes,output)?;ScanPage::decode(&output[..length],prefix,limits)}

/// Scans principal-scoped storage. # Errors Refuses authority, bounds, meter, or malformed host output.
#[cfg(target_arch="wasm32")]
pub fn scan<'a>(prefix:&[u8],cursor:&ScanCursor<'_>,limits:ScanLimits,output:&'a mut[u8])->Result<ScanPage<'a>,ProgramError>{scan_with(host::storage_scan_principal,prefix,cursor,limits,output)}
/// Drops the complete principal-scoped namespace. # Errors Refuses missing authority or meter.
#[cfg(target_arch="wasm32")]
pub fn drop_namespace()->Result<(),ProgramError>{host::storage_drop_principal()}

/// Program-shared storage operations.
///
/// Every binding addresses the shared namespace `(program)` only, which is
/// readable and writable by every principal invoking this program. The
/// namespace is visible in the type so a program cannot accidentally address
/// principal-scoped state when it needs shared state, or shared state when it
/// needs principal-scoped state.
pub mod shared {
    use super::*;

    /// A key inside this program's shared namespace.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct SharedStorageKey<'a>(&'a [u8]);

    impl<'a> SharedStorageKey<'a> {
        /// Constructs a key inside the shared namespace bound.
        ///
        /// # Errors
        ///
        /// Refuses an empty key and a key past the declared bound.
        pub const fn new(bytes: &'a [u8]) -> Result<Self, ProgramError> {
            if bytes.is_empty() {
                return Err(ProgramError::value(Field::StorageKey, Reason::Empty));
            }
            if bytes.len() > MAX_STORAGE_KEY_BYTES {
                return Err(ProgramError::value(Field::StorageKey, Reason::TooLarge));
            }
            Ok(Self(bytes))
        }

        /// Borrows the canonical key bytes.
        #[must_use]
        pub const fn bytes(self) -> &'a [u8] {
            self.0
        }
    }

    /// Reads one value from shared storage into a caller-owned buffer.
    ///
    /// Returns `None` when the key holds no value.
    ///
    /// # Errors
    ///
    /// Refuses missing `SharedStorageRead` authority, a buffer shorter than
    /// the stored value, and every meter refusal.
    #[cfg(target_arch = "wasm32")]
    pub fn read(
        key: SharedStorageKey<'_>,
        output: &mut [u8],
    ) -> Result<Option<usize>, ProgramError> {
        let status = host::storage_read_shared(key.bytes(), output)?;
        decode_read_length(status, output.len())
    }

    /// Reads one value from shared storage into a fixed-capacity buffer.
    ///
    /// # Errors
    ///
    /// Refuses missing `SharedStorageRead` authority, a buffer shorter than
    /// the stored value, and every meter refusal.
    #[cfg(target_arch = "wasm32")]
    pub fn read_into<const N: usize>(
        key: SharedStorageKey<'_>,
        output: &mut Bytes<N>,
    ) -> Result<bool, ProgramError> {
        output.clear();
        let Some(length) = read(key, output.as_mut_slice())? else {
            return Ok(false);
        };
        output.set_length(length)?;
        Ok(true)
    }

    /// Stages one value in this program's shared namespace.
    ///
    /// # Errors
    ///
    /// Refuses missing `SharedStorageWrite` authority, invalid bounds, and
    /// every meter refusal.
    #[cfg(target_arch = "wasm32")]
    pub fn write(
        key: SharedStorageKey<'_>,
        value: StorageValue<'_>,
    ) -> Result<(), ProgramError> {
        host::storage_write_shared(key.bytes(), value.bytes())?;
        Ok(())
    }

    /// Stages the deletion of one key in this program's shared namespace.
    ///
    /// # Errors
    ///
    /// Refuses missing `SharedStorageWrite` authority, invalid keys, and
    /// every meter refusal.
    #[cfg(target_arch = "wasm32")]
    pub fn delete(key: SharedStorageKey<'_>) -> Result<(), ProgramError> {
        host::storage_delete_shared(key.bytes())?;
        Ok(())
    }

    /// Scans shared storage. # Errors Refuses authority, bounds, meter, or malformed host output.
    #[cfg(target_arch="wasm32")]
    pub fn scan<'a>(prefix:&[u8],cursor:&ScanCursor<'_>,limits:ScanLimits,output:&'a mut[u8])->Result<ScanPage<'a>,ProgramError>{scan_with(host::storage_scan_shared,prefix,cursor,limits,output)}
    /// Drops the complete shared namespace. # Errors Refuses missing authority or meter.
    #[cfg(target_arch="wasm32")]
    pub fn drop_namespace()->Result<(),ProgramError>{host::storage_drop_shared()}
}

#[cfg(test)]
mod tests {
    use super::decode_read_length;

    #[test]
    fn host_read_length_cannot_exceed_guest_capacity() {
        assert_eq!(decode_read_length(0, 4), Ok(None));
        assert_eq!(decode_read_length(5, 4), Ok(Some(4)));
        assert!(decode_read_length(6, 4).is_err());
    }
}
