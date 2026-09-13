use layerx_solana_mirror_program::process_instruction;
use sha2::{Digest, Sha256};
use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    system_program,
};
use solana_program_test::{processor, ProgramTest, ProgramTestContext};
use solana_sdk::{signature::Signer, transaction::Transaction};

const ARCHIVE: &[u8] = include_bytes!("fixtures/native.archive");

fn instruction(opcode: u8, commitment: [u8; 32], tail: &[u8]) -> Vec<u8> {
    let mut data = b"LXMA".to_vec();
    data.extend_from_slice(&3_u16.to_be_bytes());
    data.push(opcode);
    data.extend_from_slice(&commitment);
    data.extend_from_slice(tail);
    data
}

async fn send(
    context: &mut ProgramTestContext,
    instructions: &[Instruction],
) -> Result<(), String> {
    let hash = context
        .get_new_latest_blockhash()
        .await
        .map_err(|error| error.to_string())?;
    let transaction = Transaction::new_signed_with_payer(
        instructions,
        Some(&context.payer.pubkey()),
        &[&context.payer],
        hash,
    );
    context
        .banks_client
        .process_transaction(transaction)
        .await
        .map_err(|error| error.to_string())
}

async fn manifest_data(context: &mut ProgramTestContext, manifest: Pubkey) -> Vec<u8> {
    context
        .banks_client
        .get_account(manifest)
        .await
        .expect("runtime account lookup")
        .expect("committed manifest account")
        .data
}

async fn qualify(wrong_digest: bool, chunk_size: usize) {
    let program = Pubkey::new_unique();
    let mut context = ProgramTest::new(
        "layerx_solana_mirror_program",
        program,
        processor!(process_instruction),
    )
    .start_with_context()
    .await;
    let publisher = context.payer.pubkey();
    let commitment = solana_program::hash::hashv(&[b"LXP/mirror/archive/v2\0", ARCHIVE]).to_bytes();
    let manifest =
        Pubkey::find_program_address(&[b"manifest", publisher.as_ref(), &commitment], &program).0;
    let mut digest: [u8; 32] = Sha256::digest(ARCHIVE).into();
    if wrong_digest {
        digest[0] ^= 1;
    }
    let chunks: Vec<_> = ARCHIVE.chunks(chunk_size).collect();
    let mut chain = [0; 32];
    for (index, chunk) in chunks.iter().enumerate() {
        chain = solana_program::hash::hashv(&[
            &chain,
            &(index as u32).to_be_bytes(),
            &Sha256::digest(chunk),
            &(chunk.len() as u32).to_be_bytes(),
        ])
        .to_bytes();
    }
    let mut begin = Vec::new();
    begin.extend_from_slice(&77_u32.to_be_bytes());
    begin.extend_from_slice(&1_u64.to_be_bytes());
    begin.extend_from_slice(&[0; 32]);
    begin.extend_from_slice(&(ARCHIVE.len() as u64).to_be_bytes());
    begin.extend_from_slice(&(chunks.len() as u32).to_be_bytes());
    begin.extend_from_slice(&digest);
    begin.extend_from_slice(&chain);
    let initialize = Instruction {
        program_id: program,
        accounts: vec![
            AccountMeta::new(publisher, true),
            AccountMeta::new(manifest, false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data: instruction(1, commitment, &begin),
    };
    send(&mut context, &[initialize.clone(), initialize])
        .await
        .expect("real system PDA creation and exact initialize retry");
    let finalize = Instruction {
        program_id: program,
        accounts: vec![
            AccountMeta::new(publisher, true),
            AccountMeta::new(manifest, false),
        ],
        data: instruction(3, commitment, &[]),
    };
    let original = manifest_data(&mut context, manifest).await;
    assert!(send(&mut context, std::slice::from_ref(&finalize))
        .await
        .is_err());
    assert_eq!(manifest_data(&mut context, manifest).await, original);
    for (index, chunk) in chunks.iter().enumerate() {
        let index_bytes = (index as u32).to_be_bytes();
        let chunk_address = Pubkey::find_program_address(
            &[b"chunk", publisher.as_ref(), &commitment, &index_bytes],
            &program,
        )
        .0;
        let mut payload = index_bytes.to_vec();
        payload.extend_from_slice(&(chunk.len() as u16).to_be_bytes());
        payload.extend_from_slice(chunk);
        payload.extend_from_slice(&Sha256::digest(chunk));
        let append = Instruction {
            program_id: program,
            accounts: vec![
                AccountMeta::new(publisher, true),
                AccountMeta::new(manifest, false),
                AccountMeta::new(chunk_address, false),
                AccountMeta::new_readonly(system_program::id(), false),
            ],
            data: instruction(2, commitment, &payload),
        };
        if index == 0 {
            let mut corrupt = append.clone();
            *corrupt.data.last_mut().expect("digest exists") ^= 1;
            assert!(send(&mut context, &[corrupt]).await.is_err());
            assert_eq!(manifest_data(&mut context, manifest).await, original);
        }
        send(&mut context, std::slice::from_ref(&append))
            .await
            .expect("real chunk PDA creation");
        let committed = manifest_data(&mut context, manifest).await;
        send(&mut context, &[append])
            .await
            .expect("exact retry without digest advancement");
        assert_eq!(manifest_data(&mut context, manifest).await, committed);
    }
    let before = manifest_data(&mut context, manifest).await;
    assert_eq!(before.len(), 334);
    assert_eq!(&before[..8], b"LXMMAN03");
    assert_eq!(&before[160..192], &before[192..224]);
    assert_eq!(before[236], 0);
    let result = send(&mut context, &[finalize.clone(), finalize]).await;
    let after = manifest_data(&mut context, manifest).await;
    if wrong_digest {
        assert!(
            result.is_err(),
            "wrong archive digest must not finalize despite exact chunk chain"
        );
        assert_eq!(after, before);
    } else {
        result.expect("actual ordered archive digest finalizes and exact finalize retry succeeds");
        assert_eq!(after[236], 1);
        let mut expected = before;
        expected[236] = 1;
        assert_eq!(after, expected);
        if chunk_size == 720 {
            if let Some(directory) = std::env::var_os("LAYERX_SOLANA_RUNTIME_FIXTURE_DIR") {
                std::fs::create_dir_all(&directory).expect("public fixture directory");
                let directory = std::path::Path::new(&directory);
                std::fs::write(directory.join("manifest"), &after).expect("public manifest");
                std::fs::write(directory.join("publisher"), publisher.to_bytes())
                    .expect("public publisher");
                std::fs::write(directory.join("commitment"), commitment)
                    .expect("public commitment");
            }
        }
    }
}

#[tokio::test]
async fn real_solana_runtime_binds_archive_digest_before_finalization() {
    qualify(true, 720).await;
    qualify(false, 720).await;
    qualify(false, 128).await;
}
