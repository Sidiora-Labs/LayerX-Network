use layerx_programs_runtime::verify_secp256k1;

#[test]
fn wycheproof_secp256k1_sha256_tc3_verifies_prehash() -> Result<(), Box<dyn std::error::Error>> {
    let digest = hex::decode("bb5a52f42f9c9261ed4361f59422a1e30036e7c32b270c8807a419feca605023")?;
    let public_key = hex::decode("04782c8ed17e3b2a783b5464f33b09652a71c678e05ec51e84e2bcfc663a3de963af9acb4280b8c7f7c42f4ef9aba6245ec1ec1712fd38a0fa96418d8cd6aa6152")?;
    let signature = hex::decode("d035ee1f17fdb0b2681b163e33c359932659990af77dca632012b30b27a057b31939d9f3b2858bc13e3474cb50e6a82be44faa71940f876c1cba4c3e989202b6")?;
    assert_eq!(verify_secp256k1(&digest, &public_key, &signature), Ok(()));
    Ok(())
}
