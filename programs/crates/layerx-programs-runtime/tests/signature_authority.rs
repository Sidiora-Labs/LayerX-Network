use layerx_programs_runtime::{verify_secp256k1, SignatureRefusal};

#[test]
fn wycheproof_secp256k1_sha256_tc3_verifies_prehash() -> Result<(), Box<dyn std::error::Error>> {
    let digest = hex::decode("bb5a52f42f9c9261ed4361f59422a1e30036e7c32b270c8807a419feca605023")?;
    let public_key = hex::decode("04782c8ed17e3b2a783b5464f33b09652a71c678e05ec51e84e2bcfc663a3de963af9acb4280b8c7f7c42f4ef9aba6245ec1ec1712fd38a0fa96418d8cd6aa6152")?;
    let signature = hex::decode("d035ee1f17fdb0b2681b163e33c359932659990af77dca632012b30b27a057b31939d9f3b2858bc13e3474cb50e6a82be44faa71940f876c1cba4c3e989202b6")?;
    assert_eq!(verify_secp256k1(&digest, &public_key, &signature), Ok(()));
    Ok(())
}

#[test]
fn wycheproof_secp256k1_sha256_tc1_rejects_high_s() -> Result<(), Box<dyn std::error::Error>> {
    let digest = hex::decode("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")?;
    let public_key = hex::decode("04782c8ed17e3b2a783b5464f33b09652a71c678e05ec51e84e2bcfc663a3de963af9acb4280b8c7f7c42f4ef9aba6245ec1ec1712fd38a0fa96418d8cd6aa6152")?;
    let signature = hex::decode("f80ae4f96cdbc9d853f83d47aae225bf407d51c56b7776cd67d0dc195d99a9dcb303e26be1f73465315221f0b331528807a1a9b6eb068ede6eebeaaa49af8a36")?;
    assert_eq!(
        verify_secp256k1(&digest, &public_key, &signature),
        Err(SignatureRefusal::VerificationFailed)
    );
    Ok(())
}

#[test]
fn wycheproof_secp256k1_sha256_tc4_verifies_prehash() -> Result<(), Box<dyn std::error::Error>> {
    let digest = hex::decode("de47c9b27eb8d300dbb5f2c353e632c393262cf06340c4fa7f1b40c4cbd36f90")?;
    let public_key = hex::decode("04782c8ed17e3b2a783b5464f33b09652a71c678e05ec51e84e2bcfc663a3de963af9acb4280b8c7f7c42f4ef9aba6245ec1ec1712fd38a0fa96418d8cd6aa6152")?;
    let signature = hex::decode("4f053f563ad34b74fd8c9934ce59e79c2eb8e6eca0fef5b323ca67d5ac7ed2384d4b05daa0719e773d8617dce5631c5fd6f59c9bdc748e4b55c970040af01be5")?;
    assert_eq!(verify_secp256k1(&digest, &public_key, &signature), Ok(()));
    Ok(())
}

#[test]
fn wycheproof_secp256k1_sha256_tc7_verifies_prehash() -> Result<(), Box<dyn std::error::Error>> {
    let digest = hex::decode("bb5a52f42f9c9261ed4361f59422a1e30036e7c32b270c8807a419feca605023")?;
    let public_key = hex::decode("04b838ff44e5bc177bf21189d0766082fc9d843226887fc9760371100b7ee20a6ff0c9d75bfba7b31a6bca1974496eeb56de357071955d83c4b1badaa0b21832e9")?;
    let signature = hex::decode("813ef79ccefa9a56f7ba805f0e478584fe5f0dd5f567bc09b5123ccbc98323656ff18a52dcc0336f7af62400a6dd9b810732baf1ff758000d6f613a556eb31ba")?;
    assert_eq!(verify_secp256k1(&digest, &public_key, &signature), Ok(()));
    Ok(())
}

#[test]
fn wycheproof_secp256k1_sha256_tc103_rejects_invalid_signature(
) -> Result<(), Box<dyn std::error::Error>> {
    let digest = hex::decode("bb5a52f42f9c9261ed4361f59422a1e30036e7c32b270c8807a419feca605023")?;
    let public_key = hex::decode("04b838ff44e5bc177bf21189d0766082fc9d843226887fc9760371100b7ee20a6ff0c9d75bfba7b31a6bca1974496eeb56de357071955d83c4b1badaa0b21832e9")?;
    let signature = hex::decode("813ef79ccefa9a56f7ba805f0e478584fe5f0dd5f567bc09b5123ccbc98323e56ff18a52dcc0336f7af62400a6dd9b810732baf1ff758000d6f613a556eb31ba")?;
    assert_eq!(
        verify_secp256k1(&digest, &public_key, &signature),
        Err(SignatureRefusal::VerificationFailed)
    );
    Ok(())
}

#[test]
fn wycheproof_secp256k1_sha256_tc104_rejects_invalid_signature(
) -> Result<(), Box<dyn std::error::Error>> {
    let digest = hex::decode("bb5a52f42f9c9261ed4361f59422a1e30036e7c32b270c8807a419feca605023")?;
    let public_key = hex::decode("04b838ff44e5bc177bf21189d0766082fc9d843226887fc9760371100b7ee20a6ff0c9d75bfba7b31a6bca1974496eeb56de357071955d83c4b1badaa0b21832e9")?;
    let signature = hex::decode("00813ef79ccefa9a56f7ba805f0e478584fe5f0dd5f567bc09b5123ccbc983236ff18a52dcc0336f7af62400a6dd9b810732baf1ff758000d6f613a556eb31ba")?;
    assert_eq!(
        verify_secp256k1(&digest, &public_key, &signature),
        Err(SignatureRefusal::VerificationFailed)
    );
    Ok(())
}
