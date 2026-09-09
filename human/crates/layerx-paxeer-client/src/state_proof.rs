use sha2::{Digest as _, Sha256};

const LEAF: &[u8] = b"LXP/v1/state-leaf\0";
const NODE: &[u8] = b"LXP/v1/state-node\0";
const MAX_KEY: usize = 129;
const MAX_VALUE: usize = 1_048_576;
const MAX_DEPTH: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StateProofError {
    Version,
    Encoding,
    Module,
    Path,
    Root,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StateWitness {
    pub module_id: u16,
    pub key: Vec<u8>,
    pub value: Vec<u8>,
    pub account_path: Option<AccountPath>,
    pub leaf_index_a: u32,
    pub leaf_count_a: u32,
    pub siblings_a: Vec<[u8; 32]>,
    pub leaf_count_b: u32,
    pub siblings_b: Vec<[u8; 32]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountPath {
    pub index: u32,
    pub count: u32,
    pub siblings: Vec<[u8; 32]>,
}

impl StateWitness {
    /// # Errors
    /// Refuses unsupported versions, noncanonical paths, lengths and trailing bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self, StateProofError> {
        let mut reader = Reader(bytes);
        if u16::from_be_bytes(reader.array()?) != 2 {
            return Err(StateProofError::Version);
        }
        let module_id = u16::from_be_bytes(reader.array()?);
        if module_id > 9 {
            return Err(StateProofError::Module);
        }
        let key = reader.vector(MAX_KEY)?;
        let value = reader.vector(MAX_VALUE)?;
        let account_path = if module_id == 0 && key.len() == 33 && key[0] == 4 {
            Some(AccountPath {
                index: u32::from_be_bytes(reader.array()?),
                count: u32::from_be_bytes(reader.array()?),
                siblings: reader.path()?,
            })
        } else {
            None
        };
        let leaf_index_a = u32::from_be_bytes(reader.array()?);
        let leaf_count_a = u32::from_be_bytes(reader.array()?);
        let siblings_a = reader.path()?;
        let leaf_count_b = u32::from_be_bytes(reader.array()?);
        let siblings_b = reader.path()?;
        if !reader.0.is_empty() {
            return Err(StateProofError::Encoding);
        }
        let witness = Self {
            module_id,
            key,
            value,
            account_path,
            leaf_index_a,
            leaf_count_a,
            siblings_a,
            leaf_count_b,
            siblings_b,
        };
        witness.root()?;
        Ok(witness)
    }

    /// # Errors
    /// Refuses noncanonical module identifiers, lengths and paths.
    pub fn encode(&self) -> Result<Vec<u8>, StateProofError> {
        self.root()?;
        let mut out = 2_u16.to_be_bytes().to_vec();
        out.extend_from_slice(&self.module_id.to_be_bytes());
        for bytes in [&self.key, &self.value] {
            let len = u32::try_from(bytes.len()).map_err(|_| StateProofError::Encoding)?;
            out.extend_from_slice(&len.to_be_bytes());
            out.extend_from_slice(bytes);
        }
        if let Some(path) = &self.account_path {
            out.extend_from_slice(&path.index.to_be_bytes());
            out.extend_from_slice(&path.count.to_be_bytes());
            append_path(&mut out, &path.siblings)?;
        }
        out.extend_from_slice(&self.leaf_index_a.to_be_bytes());
        out.extend_from_slice(&self.leaf_count_a.to_be_bytes());
        append_path(&mut out, &self.siblings_a)?;
        out.extend_from_slice(&self.leaf_count_b.to_be_bytes());
        append_path(&mut out, &self.siblings_b)?;
        Ok(out)
    }

    /// # Errors
    /// Refuses noncanonical paths and a root unequal to the registered composite root.
    pub fn verify(&self, state_root: [u8; 32]) -> Result<(), StateProofError> {
        if self.root()? == state_root {
            Ok(())
        } else {
            Err(StateProofError::Root)
        }
    }

    /// # Errors
    /// Refuses out-of-range modules, lengths, indices, depths and odd-node siblings.
    pub fn root(&self) -> Result<[u8; 32], StateProofError> {
        if self.module_id > 9 || !(9..=10).contains(&self.leaf_count_b) {
            return Err(StateProofError::Module);
        }
        if self.key.is_empty() || self.key.len() > MAX_KEY || self.value.len() > MAX_VALUE {
            return Err(StateProofError::Encoding);
        }
        let is_account = self.module_id == 0 && self.key.len() == 33 && self.key[0] == 4;
        if is_account != self.account_path.is_some() {
            return Err(StateProofError::Encoding);
        }
        let mut node = leaf(&self.key, &self.value)?;
        if let Some(path) = &self.account_path {
            node = fold(node, path.index, path.count, &path.siblings)?;
            node = leaf(b"account-tree", &node)?;
        }
        let subtree = fold(node, self.leaf_index_a, self.leaf_count_a, &self.siblings_a)?;
        fold(
            leaf(&self.module_id.to_be_bytes(), &subtree)?,
            u32::from(self.module_id),
            self.leaf_count_b,
            &self.siblings_b,
        )
    }
}

fn leaf(key: &[u8], value: &[u8]) -> Result<[u8; 32], StateProofError> {
    let key_len = u32::try_from(key.len()).map_err(|_| StateProofError::Encoding)?;
    let value_len = u32::try_from(value.len()).map_err(|_| StateProofError::Encoding)?;
    Ok(hash(&[
        LEAF,
        &key_len.to_be_bytes(),
        &value_len.to_be_bytes(),
        key,
        value,
    ]))
}
fn hash(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}
fn fold(
    mut node: [u8; 32],
    mut index: u32,
    mut count: u32,
    siblings: &[[u8; 32]],
) -> Result<[u8; 32], StateProofError> {
    if count == 0 || index >= count || siblings.len() > MAX_DEPTH {
        return Err(StateProofError::Path);
    }
    for sibling in siblings {
        if count <= 1 || ((index ^ 1) >= count && sibling != &node) {
            return Err(StateProofError::Path);
        }
        node = if index & 1 == 0 {
            hash(&[NODE, &node, sibling])
        } else {
            hash(&[NODE, sibling, &node])
        };
        index /= 2;
        count = count.div_ceil(2);
    }
    if count == 1 {
        Ok(node)
    } else {
        Err(StateProofError::Path)
    }
}
fn append_path(out: &mut Vec<u8>, path: &[[u8; 32]]) -> Result<(), StateProofError> {
    out.push(u8::try_from(path.len()).map_err(|_| StateProofError::Path)?);
    for sibling in path {
        out.extend_from_slice(sibling);
    }
    Ok(())
}
struct Reader<'a>(&'a [u8]);
impl Reader<'_> {
    fn array<const N: usize>(&mut self) -> Result<[u8; N], StateProofError> {
        let out = self
            .0
            .get(..N)
            .ok_or(StateProofError::Encoding)?
            .try_into()
            .map_err(|_| StateProofError::Encoding)?;
        self.0 = &self.0[N..];
        Ok(out)
    }
    fn vector(&mut self, maximum: usize) -> Result<Vec<u8>, StateProofError> {
        let len = usize::try_from(u32::from_be_bytes(self.array()?))
            .map_err(|_| StateProofError::Encoding)?;
        if len > maximum {
            return Err(StateProofError::Encoding);
        }
        let out = self.0.get(..len).ok_or(StateProofError::Encoding)?.to_vec();
        self.0 = &self.0[len..];
        Ok(out)
    }
    fn path(&mut self) -> Result<Vec<[u8; 32]>, StateProofError> {
        let [depth] = self.array()?;
        if usize::from(depth) > MAX_DEPTH {
            return Err(StateProofError::Path);
        }
        (0..depth).map(|_| self.array()).collect()
    }
}

impl std::fmt::Display for StateProofError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "native state proof: {self:?}")
    }
}
impl std::error::Error for StateProofError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeEvidence {
    pub request_anchor: [u8; 32],
    pub inclusion_checkpoint: [u8; 32],
    pub network_id: u32,
    pub witness: Vec<u8>,
    pub recipient_signature: Vec<u8>,
}

impl NativeEvidence {
    /// # Errors
    /// Refuses empty domains, malformed signatures and mismatched native state roots.
    pub fn decoded(&self, root: [u8; 32]) -> Result<StateWitness, StateProofError> {
        if self.request_anchor == [0; 32]
            || self.inclusion_checkpoint == [0; 32]
            || self.network_id == 0
            || !matches!(self.recipient_signature.len(), 0 | 64)
        {
            return Err(StateProofError::Encoding);
        }
        let witness = StateWitness::decode(&self.witness)?;
        witness.verify(root)?;
        Ok(witness)
    }
}
