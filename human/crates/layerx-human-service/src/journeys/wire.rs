//! Canonical bounded primitives for the privileged movement-provider wire.

pub(super) const MAX_TEXT_BYTES: usize = 512;
pub(super) const MAX_PROOF_ITEMS: usize = 256;

pub(super) struct Writer(Vec<u8>);

impl Writer {
    pub(super) fn new(tag: u8) -> Self {
        Self(vec![1, tag])
    }
    pub(super) fn fixed(&mut self, value: &[u8]) {
        self.0.extend_from_slice(value);
    }
    pub(super) fn u16(&mut self, value: u16) {
        self.fixed(&value.to_be_bytes());
    }
    pub(super) fn u32(&mut self, value: u32) {
        self.fixed(&value.to_be_bytes());
    }
    pub(super) fn u64(&mut self, value: u64) {
        self.fixed(&value.to_be_bytes());
    }
    pub(super) fn u128(&mut self, value: u128) {
        self.fixed(&value.to_be_bytes());
    }
    pub(super) fn boolean(&mut self, value: bool) {
        self.0.push(u8::from(value));
    }
    pub(super) fn text(&mut self, value: &str) -> Result<(), ()> {
        if value.is_empty() || value.len() > MAX_TEXT_BYTES || value.len() > u16::MAX as usize {
            return Err(());
        }
        self.u16(value.len() as u16);
        self.fixed(value.as_bytes());
        Ok(())
    }
    pub(super) fn finish(self) -> Vec<u8> {
        self.0
    }
}

pub(super) struct Reader<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl<'a> Reader<'a> {
    pub(super) fn new(bytes: &'a [u8], tag: u8) -> Result<Self, ()> {
        if bytes.get(..2) != Some([1, tag].as_slice()) {
            return Err(());
        }
        Ok(Self { bytes, cursor: 2 })
    }
    pub(super) fn fixed<const N: usize>(&mut self) -> Result<[u8; N], ()> {
        let end = self.cursor.checked_add(N).ok_or(())?;
        let value: [u8; N] = self
            .bytes
            .get(self.cursor..end)
            .ok_or(())?
            .try_into()
            .map_err(|_| ())?;
        self.cursor = end;
        Ok(value)
    }
    pub(super) fn u16(&mut self) -> Result<u16, ()> {
        Ok(u16::from_be_bytes(self.fixed()?))
    }
    pub(super) fn u32(&mut self) -> Result<u32, ()> {
        Ok(u32::from_be_bytes(self.fixed()?))
    }
    pub(super) fn u64(&mut self) -> Result<u64, ()> {
        Ok(u64::from_be_bytes(self.fixed()?))
    }
    pub(super) fn u128(&mut self) -> Result<u128, ()> {
        Ok(u128::from_be_bytes(self.fixed()?))
    }
    pub(super) fn boolean(&mut self) -> Result<bool, ()> {
        match self.fixed::<1>()?[0] {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(()),
        }
    }
    pub(super) fn text(&mut self) -> Result<String, ()> {
        let length = usize::from(self.u16()?);
        if length == 0 || length > MAX_TEXT_BYTES {
            return Err(());
        }
        let end = self.cursor.checked_add(length).ok_or(())?;
        let value = std::str::from_utf8(self.bytes.get(self.cursor..end).ok_or(())?)
            .map_err(|_| ())?
            .to_owned();
        self.cursor = end;
        Ok(value)
    }
    pub(super) fn finish(self) -> Result<(), ()> {
        if self.cursor == self.bytes.len() {
            Ok(())
        } else {
            Err(())
        }
    }
}
