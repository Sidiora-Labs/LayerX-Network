use layerx_proof::merkle::Proof;
use layerx_wire::encode::Encoder;

/// # Panics
/// Panics if a proof violates its original canonical depth or byte bound.
#[must_use]
pub fn native_merkle_proof(proof: &Proof) -> Vec<u8> {
    let mut writer = Encoder::new(1_041);
    let mut write = || {
        writer.structure_header(0x4d50)?;
        writer.u32(proof.leaf_index())?;
        writer.u32(proof.leaf_count())?;
        writer.u8(u8::try_from(proof.siblings().len()).unwrap_or_else(|e| panic!("depth: {e}")))?;
        writer.bytes(&proof.siblings().concat(), 1_024)
    };
    write().unwrap_or_else(|e| panic!("native proof: {e:?}"));
    writer.finish()
}
