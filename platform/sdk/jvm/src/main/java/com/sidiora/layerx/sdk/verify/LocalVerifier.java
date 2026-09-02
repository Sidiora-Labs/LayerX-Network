package com.sidiora.layerx.sdk.verify;

import com.sidiora.layerx.sdk.PlatformSdkException;
import com.sidiora.layerx.sdk.verify.GeneratedReceiptContract.ReceiptCheck;
import java.io.ByteArrayOutputStream;
import java.math.BigInteger;
import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;
import java.security.KeyFactory;
import java.security.MessageDigest;
import java.security.Signature;
import java.security.spec.X509EncodedKeySpec;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.HashSet;
import java.util.List;
import java.util.Objects;
import java.util.Set;
import org.bouncycastle.crypto.ec.CustomNamedCurves;
import org.bouncycastle.crypto.digests.KeccakDigest;
import org.bouncycastle.crypto.params.ECDomainParameters;
import org.bouncycastle.crypto.params.ECPublicKeyParameters;
import org.bouncycastle.crypto.signers.ECDSASigner;
import org.bouncycastle.math.ec.ECAlgorithms;
import org.bouncycastle.math.ec.ECPoint;

/** Trustless 402LXP receipt, Merkle, batch, and checkpoint verification. */
public final class LocalVerifier {
    private LocalVerifier() {}

    private static final byte[] MERKLE_LEAF_DOMAIN = ascii("LXP/v1/merkle-leaf\0");
    private static final byte[] MERKLE_INTERNAL_DOMAIN = ascii("LXP/v1/merkle-internal\0");
    private static final byte[] BATCH_HEADER_DOMAIN = ascii("LXP/v1/batch-header\0");
    private static final byte[] RECEIPT_DOMAIN = ascii("LXP/v1/receipt\0");
    private static final byte[] CHECKPOINT_DOMAIN = ascii("LXP/v2/checkpoint-certificate\0");
    private static final byte[] GUARANTOR_ATTESTATION_DOMAIN = ascii("LXP/v2/guarantor-attestation\0");
    private static final byte[] ED25519_X509_PREFIX = hex("302a300506032b6570032100");
    private static final int BATCH_HEADER_BYTES = 354;
    private static final int MAX_MESSAGE_BYTES = 1_048_576;
    private static final int MAX_EFFECTS = 512;
    private static final int MAX_EFFECT_BODY = 256;
    private static final int ALL_AVAILABILITY_CLASSES = 0x1f;
    private static final int CURRENT_PROTOCOL_VERSION = 2;
    private static final long PROGRAM_OUTCOME_V1 = GeneratedReceiptContract.PROGRAM_OUTCOME_V1;
    private static final long PROGRAM_OUTCOME_V2 = GeneratedReceiptContract.PROGRAM_OUTCOME_V2;
    private static final long PROGRAM_OUTCOME_V3 = GeneratedReceiptContract.PROGRAM_OUTCOME_V3;
    private static final BigInteger MAX_U128 = BigInteger.ONE.shiftLeft(128).subtract(BigInteger.ONE);

    public record MerkleProof(long leafIndex, long leafCount, List<byte[]> siblings) {
        public MerkleProof { siblings = List.copyOf(siblings); }
    }
    public record BatchHeader(int protocolVersion, long networkId, BigInteger epoch, BigInteger batchNumber,
        BigInteger firstSequence, BigInteger lastSequence, byte[] previousStateRoot, byte[] resultingStateRoot,
        byte[] activityMerkleRoot, byte[] receiptMerkleRoot, byte[] eventMerkleRoot,
        byte[] dataAvailabilityRoot, byte[] oracleRoot, BigInteger timestampMs, byte[] sequencerId) {}
    public record SequencerAuthorization(byte[] sequencerId, byte[] publicKey,
        BigInteger firstBatchNumber, BigInteger lastBatchNumber) {}
    public enum InclusionKind { ACTIVITY, RECEIPT, EVENT, STATE }
    public enum VerificationLevel {
        SEQUENCER_SIGNED("sequencer-signed"), BATCH_INCLUDED("batch-included"),
        STATE_PROVEN("state-proven"), CHECKPOINT_FINALISED("checkpoint-finalised"),
        SETTLEMENT_ANCHORED("settlement-anchored");
        private final String wire;
        VerificationLevel(String wire) { this.wire = wire; }
        public String wire() { return wire; }
    }
    public record InclusionVerification(VerificationLevel level, BatchHeader header,
        byte[] headerDigest, byte[] root) {}
    public record CheckpointAttestation(int protocolVersion, long networkId, BigInteger paxeerChainId,
        byte[] settlementContract, BigInteger epoch, byte[] checkpointId, byte[] checkpointHash,
        byte[] guarantorId, BigInteger batchNumber, byte[] dataAvailabilityRoot, boolean replayed,
        boolean dataPossessed, int availabilityClassMask, BigInteger attestedAtMs, byte[] signer,
        byte[] signature, int signatureV) {}
    public record GuarantorKey(byte[] guarantorId, byte[] publicKey, boolean bonded) {}
    public record CheckpointCertificate(byte[] canonicalHeader, byte[] validityProof,
        List<CheckpointAttestation> attestations, int threshold, byte[] settlementReference) {
        public CheckpointCertificate { attestations = List.copyOf(attestations); }
    }
    public record CheckpointVerificationInput(CheckpointCertificate certificate, List<GuarantorKey> bondedSet,
        byte[] registeredCheckpointId, BigInteger expectedPaxeerChainId, byte[] expectedSettlementContract,
        byte[] registeredSettlementReference, boolean availabilityObtained) {
        public CheckpointVerificationInput { bondedSet = List.copyOf(bondedSet); }
    }
    public record CheckpointVerification(VerificationLevel level, byte[] checkpointId, int achieved,
        int required, BatchHeader header) {}
    @FunctionalInterface public interface LocalSignatureVerifier {
        boolean verifyRecoverableSecp256k1(byte[] publicKey, byte[] signature, int signatureV,
            byte[] signer, byte[] digest);
    }
    public record ReceiptEffect(int moduleId, int ordinal, int eventType, int kind, boolean monetary,
        byte[] transferSetRoot, byte[] body) {}
    public record ProgramReceiptOutcome(int encodingVersion, int terminalKind, int resultCode,
        int runtimeVersion, int abiVersion, long feeScheduleVersion, long meteringScheduleVersion,
        BigInteger cpuFuel, BigInteger memoryBytes, BigInteger storageReadBytes,
        BigInteger storageWriteBytes, long outputValues, BigInteger outputBytes,
        BigInteger occupancyByteBatches, BigInteger occupancyFeeUnits, List<BigInteger> feeSchedulePrices,
        byte[] occupancyAssetId, byte[] occupancyEvidenceDigest, byte[] occupancyTransferRoot,
        BigInteger feeUnits, byte[] callGraphRoot, byte[] terminalPayloadRoot, byte[] transferRoot) {
        public ProgramReceiptOutcome { feeSchedulePrices = List.copyOf(feeSchedulePrices); }
    }
    public record ProtocolReceipt(int protocolVersion, byte[] activityId, BigInteger globalSequence,
        byte[] previousStateRoot, byte[] resultingStateRoot, byte[] activityRoot, int resultCode,
        List<ReceiptEffect> effects, BigInteger feeCharged, byte[] batchId, int moduleId,
        long moduleVersion, long parameterVersion, int operation, byte[] asset, BigInteger amount,
        byte[] from, BigInteger fromBalanceBefore, BigInteger fromBalanceAfter, BigInteger fromSequence,
        byte[] to, BigInteger toBalanceBefore, BigInteger toBalanceAfter, byte[] transferSetRoot,
        byte[] authorizationHash, byte[] contextHash, BigInteger timestamp,
        ProgramReceiptOutcome programOutcome, byte[] sequencerSignature) {
        public ProtocolReceipt { effects = List.copyOf(effects); }
    }
    public record AuthorizedReceiptBatch(byte[] batchId, byte[] asset, byte[] previousStateRoot,
        byte[] resultingStateRoot, byte[] sequencerPublicKey) {}
    public record ReceiptVerification(VerificationLevel level, ProtocolReceipt receipt,
        byte[] canonicalBytes, byte[] receiptDigest) {}
    public record ReceiptInclusionVerification(ReceiptVerification receipt,
        InclusionVerification inclusion) {}
    private record DecodedReceipt(ProtocolReceipt receipt, byte[] unsignedBytes) {}

    public static void verifyMerkleInclusion(byte[] canonicalLeaf, MerkleProof proof, byte[] expectedRoot) {
        Objects.requireNonNull(canonicalLeaf, "canonicalLeaf");
        Objects.requireNonNull(proof, "proof");
        if (proof.leafCount() <= 0 || proof.leafCount() > 0xffff_ffffL || proof.leafIndex() < 0
                || proof.leafIndex() >= proof.leafCount() || proof.siblings().size() > 32
                || proof.siblings().size() != proofDepth(proof.leafCount())) fail();
        byte[] current = sha256(MERKLE_LEAF_DOMAIN, canonicalLeaf);
        long index = proof.leafIndex();
        long levelCount = proof.leafCount();
        for (byte[] siblingValue : proof.siblings()) {
            byte[] sibling = exact(siblingValue, 32);
            if ((index ^ 1) >= levelCount && !equal(sibling, current)) fail();
            current = (index & 1) == 0
                ? sha256(MERKLE_INTERNAL_DOMAIN, current, sibling)
                : sha256(MERKLE_INTERNAL_DOMAIN, sibling, current);
            index /= 2;
            levelCount = (levelCount + 1) / 2;
        }
        if (!equal(current, exact(expectedRoot, 32))) fail();
    }

    public static BatchHeader decodeBatchHeader(byte[] canonicalHeader) {
        if (canonicalHeader == null || canonicalHeader.length != BATCH_HEADER_BYTES) fail();
        Decoder decoder = new Decoder(canonicalHeader);
        if (decoder.u16() != 1 || decoder.u16() != 0x1701 || decoder.u8() != 15) fail();
        field(decoder, 1); int protocolVersion = decoder.u16();
        field(decoder, 2); long networkId = decoder.u32();
        field(decoder, 3); BigInteger epoch = decoder.integer(8);
        field(decoder, 4); BigInteger batchNumber = decoder.integer(8);
        field(decoder, 5); BigInteger firstSequence = decoder.integer(8);
        field(decoder, 6); BigInteger lastSequence = decoder.integer(8);
        field(decoder, 7); byte[] previousStateRoot = decoder.bounded(32);
        field(decoder, 8); byte[] resultingStateRoot = decoder.bounded(32);
        field(decoder, 9); byte[] activityMerkleRoot = decoder.bounded(32);
        field(decoder, 10); byte[] receiptMerkleRoot = decoder.bounded(32);
        field(decoder, 11); byte[] eventMerkleRoot = decoder.bounded(32);
        field(decoder, 12); byte[] dataAvailabilityRoot = decoder.bounded(32);
        field(decoder, 13); byte[] oracleRoot = decoder.bounded(32);
        field(decoder, 14); BigInteger timestampMs = decoder.integer(8);
        field(decoder, 15); byte[] sequencerId = decoder.bounded(32);
        decoder.finish();
        return new BatchHeader(protocolVersion, networkId, epoch, batchNumber, firstSequence, lastSequence,
            previousStateRoot, resultingStateRoot, activityMerkleRoot, receiptMerkleRoot, eventMerkleRoot,
            dataAvailabilityRoot, oracleRoot, timestampMs, sequencerId);
    }

    public static InclusionVerification verifyBatchInclusion(InclusionKind kind, byte[] canonicalLeaf,
        MerkleProof proof, byte[] canonicalHeader, byte[] headerSignature, SequencerAuthorization authorization) {
        BatchHeader header = decodeBatchHeader(canonicalHeader);
        if (header.protocolVersion() != CURRENT_PROTOCOL_VERSION) fail();
        if (header.batchNumber().compareTo(authorization.firstBatchNumber()) < 0
                || header.batchNumber().compareTo(authorization.lastBatchNumber()) > 0
                || !equal(header.sequencerId(), exact(authorization.sequencerId(), 32))) fail();
        byte[] digest = sha256(BATCH_HEADER_DOMAIN, canonicalHeader);
        if (!verifyEd25519(authorization.publicKey(), headerSignature, digest)) fail();
        byte[] root = switch (kind) {
            case ACTIVITY -> header.activityMerkleRoot();
            case RECEIPT -> header.receiptMerkleRoot();
            case EVENT -> header.eventMerkleRoot();
            case STATE -> header.resultingStateRoot();
        };
        verifyMerkleInclusion(canonicalLeaf, proof, root);
        return new InclusionVerification(kind == InclusionKind.STATE ? VerificationLevel.STATE_PROVEN
            : VerificationLevel.BATCH_INCLUDED, header, digest, root.clone());
    }

    public static CheckpointVerification verifyCheckpoint(CheckpointVerificationInput input,
                                                            LocalSignatureVerifier signatures) {
        Objects.requireNonNull(input, "input"); Objects.requireNonNull(signatures, "signatures");
        CheckpointCertificate certificate = input.certificate();
        if (!input.availabilityObtained() || certificate.validityProof().length > 0xffff_ffffL) fail();
        BatchHeader header = decodeBatchHeader(certificate.canonicalHeader());
        if (header.protocolVersion() != CURRENT_PROTOCOL_VERSION) fail();
        byte[] checkpointId = sha256(CHECKPOINT_DOMAIN, certificate.canonicalHeader(),
            u32(certificate.validityProof().length), certificate.validityProof());
        byte[] expectedSettlementContract = exact(input.expectedSettlementContract(), 20);
        if (!equal(checkpointId, exact(input.registeredCheckpointId(), 32)) || certificate.threshold() <= 0
                || input.expectedPaxeerChainId() == null || input.expectedPaxeerChainId().signum() <= 0
                || allZero(expectedSettlementContract)) fail();
        Set<String> seen = new HashSet<>();
        int achieved = 0;
        for (CheckpointAttestation attestation : certificate.attestations()) {
            byte[] guarantorId = exact(attestation.guarantorId(), 32);
            byte[] attestationSettlementContract = exact(attestation.settlementContract(), 20);
            if (!seen.add(java.util.HexFormat.of().formatHex(guarantorId))
                    || attestation.protocolVersion() != header.protocolVersion()
                    || attestation.networkId() != header.networkId()
                    || !attestation.epoch().equals(header.epoch())
                    || !attestation.paxeerChainId().equals(input.expectedPaxeerChainId())
                    || !equal(attestationSettlementContract, expectedSettlementContract)
                    || !equal(attestation.checkpointId(), checkpointId)
                    || !equal(attestation.checkpointHash(), checkpointId)
                    || !attestation.batchNumber().equals(header.batchNumber())
                    || !equal(attestation.dataAvailabilityRoot(), header.dataAvailabilityRoot())
                    || !attestation.replayed() || !attestation.dataPossessed()
                    || attestation.availabilityClassMask() != ALL_AVAILABILITY_CLASSES
                    || attestation.attestedAtMs().signum() <= 0
                    || allZero(exact(attestation.signer(), 20))
                    || (attestation.signatureV() != 27 && attestation.signatureV() != 28)) fail();
            GuarantorKey member = input.bondedSet().stream().filter(candidate -> candidate.bonded()
                && equal(candidate.guarantorId(), guarantorId)).findFirst().orElseThrow(LocalVerifier::failure);
            byte[] digest = sha256(GUARANTOR_ATTESTATION_DOMAIN, attestationMessage(attestation));
            if (!signatures.verifyRecoverableSecp256k1(exact(member.publicKey(), 33),
                    exact(attestation.signature(), 64), attestation.signatureV(),
                    exact(attestation.signer(), 20), digest)) fail();
            achieved++;
        }
        if (achieved < certificate.threshold()) fail();
        byte[] settlement = certificate.settlementReference();
        if (settlement != null && (settlement.length == 0 || input.registeredSettlementReference() == null
                || !equal(settlement, input.registeredSettlementReference()))) fail();
        return new CheckpointVerification(settlement == null ? VerificationLevel.CHECKPOINT_FINALISED
            : VerificationLevel.SETTLEMENT_ANCHORED, checkpointId, achieved, certificate.threshold(), header);
    }

    public static CheckpointVerification verifyCheckpoint(CheckpointVerificationInput input) {
        return verifyCheckpoint(input, LocalVerifier::verifyRecoverableSecp256k1);
    }

    /** Production recoverable secp256k1 verifier for registered EVM attestations. */
    public static boolean verifyRecoverableSecp256k1(byte[] publicKey, byte[] signature,
                                                       int signatureV, byte[] signer, byte[] digest) {
        try {
            exact(publicKey, 33); exact(signature, 64); exact(signer, 20); exact(digest, 32);
            if (signatureV != 27 && signatureV != 28) return false;
            var params = CustomNamedCurves.getByName("secp256k1");
            var domain = new ECDomainParameters(params.getCurve(), params.getG(), params.getN(), params.getH());
            var key = new ECPublicKeyParameters(params.getCurve().decodePoint(publicKey), domain);
            var r = new BigInteger(1, Arrays.copyOfRange(signature, 0, 32));
            var s = new BigInteger(1, Arrays.copyOfRange(signature, 32, 64));
            if (r.signum() <= 0 || r.compareTo(params.getN()) >= 0 || s.signum() <= 0
                    || s.compareTo(params.getN().shiftRight(1)) > 0) return false;
            var verifier = new ECDSASigner();
            verifier.init(false, key);
            if (!verifier.verifySignature(digest, r, s)) return false;
            byte[] compressed = new byte[33];
            compressed[0] = (byte)(2 + (signatureV - 27));
            byte[] x = unsigned(r, 32);
            System.arraycopy(x, 0, compressed, 1, x.length);
            ECPoint recoveredR = params.getCurve().decodePoint(compressed);
            if (!recoveredR.multiply(params.getN()).isInfinity()) return false;
            BigInteger inverseR = r.modInverse(params.getN());
            BigInteger e = new BigInteger(1, digest);
            ECPoint recovered = ECAlgorithms.sumOfTwoMultiplies(
                params.getG(), e.negate().mod(params.getN()).multiply(inverseR).mod(params.getN()),
                recoveredR, s.multiply(inverseR).mod(params.getN())).normalize();
            if (!equal(recovered.getEncoded(true), publicKey)) return false;
            byte[] uncompressed = recovered.getEncoded(false);
            KeccakDigest keccak = new KeccakDigest(256);
            keccak.update(uncompressed, 1, uncompressed.length - 1);
            byte[] addressHash = new byte[32];
            keccak.doFinal(addressHash, 0);
            return equal(Arrays.copyOfRange(addressHash, 12, 32), signer);
        } catch (RuntimeException error) {
            return false;
        }
    }

    public static ReceiptVerification verifyReceiptOutcome(byte[] canonicalReceipt,
                                                              AuthorizedReceiptBatch authorized) {
        DecodedReceipt decoded = decodeProtocolReceipt(canonicalReceipt);
        ProtocolReceipt receipt = decoded.receipt();
        if (receipt.protocolVersion() != CURRENT_PROTOCOL_VERSION) fail(ReceiptCheck.PROTOCOL_VERSION);
        if (receipt.operation() == 0) fail(ReceiptCheck.OPERATION);
        if (allZero(receipt.activityId())) fail(ReceiptCheck.ACTIVITY_ID);
        if (allZero(receipt.asset())) fail(ReceiptCheck.ASSET);
        if (!equal(receipt.batchId(), exact(authorized.batchId(), 32))) fail(ReceiptCheck.BATCH_ID);
        if (!equal(receipt.asset(), exact(authorized.asset(), 32))) fail(ReceiptCheck.ASSET);
        if (!equal(receipt.previousStateRoot(), exact(authorized.previousStateRoot(), 32)))
            fail(ReceiptCheck.PREVIOUS_STATE_ROOT);
        if (!equal(receipt.resultingStateRoot(), exact(authorized.resultingStateRoot(), 32)))
            fail(ReceiptCheck.RESULTING_STATE_ROOT);
        if (receipt.resultCode() == 0) {
            BigInteger expectedFrom = receipt.fromBalanceBefore().subtract(receipt.amount());
            BigInteger expectedTo = receipt.toBalanceBefore().add(receipt.amount());
            if (receipt.fromBalanceBefore().compareTo(receipt.amount()) < 0
                    || !expectedFrom.equals(receipt.fromBalanceAfter())) fail(ReceiptCheck.DEBIT_BALANCE);
            if (expectedTo.compareTo(MAX_U128) > 0 || !expectedTo.equals(receipt.toBalanceAfter()))
                fail(ReceiptCheck.CREDIT_BALANCE);
        }
        byte[] digest = sha256(RECEIPT_DOMAIN, decoded.unsignedBytes());
        if (!verifyEd25519(authorized.sequencerPublicKey(), receipt.sequencerSignature(), digest))
            fail(ReceiptCheck.SEQUENCER_SIGNATURE);
        return new ReceiptVerification(VerificationLevel.SEQUENCER_SIGNED, receipt,
            canonicalReceipt.clone(), digest);
    }

    public static ReceiptVerification verifyReceipt(byte[] canonicalReceipt, AuthorizedReceiptBatch authorized) {
        ReceiptVerification verified = verifyReceiptOutcome(canonicalReceipt, authorized);
        if (verified.receipt().resultCode() != 0) fail(ReceiptCheck.RESULT_CODE);
        return verified;
    }

    public static ReceiptInclusionVerification verifyReceiptInBatch(byte[] canonicalReceipt,
        AuthorizedReceiptBatch authorizedReceipt, MerkleProof proof, byte[] canonicalHeader,
        byte[] headerSignature, SequencerAuthorization sequencerAuthorization) {
        ReceiptVerification receipt = verifyReceipt(canonicalReceipt, authorizedReceipt);
        InclusionVerification inclusion = verifyBatchInclusion(InclusionKind.RECEIPT, canonicalReceipt,
            proof, canonicalHeader, headerSignature, sequencerAuthorization);
        if (!equal(inclusion.header().previousStateRoot(), authorizedReceipt.previousStateRoot())
                || !equal(inclusion.header().resultingStateRoot(), authorizedReceipt.resultingStateRoot())
                || !equal(inclusion.header().sequencerId(), sequencerAuthorization.sequencerId())) fail();
        return new ReceiptInclusionVerification(receipt, inclusion);
    }

    private static DecodedReceipt decodeProtocolReceipt(byte[] canonicalReceipt) {
        try {
            return decodeProtocolReceiptInner(canonicalReceipt);
        } catch (PlatformSdkException error) {
            if (error.receiptCheck() != null) throw error;
            throw failure(ReceiptCheck.DECODE);
        }
    }

    private static DecodedReceipt decodeProtocolReceiptInner(byte[] canonicalReceipt) {
        if (canonicalReceipt == null || canonicalReceipt.length == 0 || canonicalReceipt.length > MAX_MESSAGE_BYTES)
            fail(ReceiptCheck.RECEIPT_SHAPE);
        Decoder d = new Decoder(canonicalReceipt);
        int envelopeVersion = d.u16();
        if ((envelopeVersion != 1 && envelopeVersion != 2) || d.u16() != 0x5201)
            fail(ReceiptCheck.DECODE);
        int protocolVersion = d.u16();
        if (protocolVersion != envelopeVersion) fail(ReceiptCheck.PROTOCOL_VERSION);
        byte[] activityId = d.bounded(32); BigInteger globalSequence = d.integer(8);
        byte[] previousStateRoot = d.bounded(32); byte[] resultingStateRoot = d.bounded(32);
        byte[] activityRoot = d.bounded(32); int resultCode = d.i32(); long effectCount = d.u32();
        if (effectCount > MAX_EFFECTS) fail();
        List<ReceiptEffect> effects = new ArrayList<>((int) effectCount);
        for (int i = 0; i < effectCount; i++) {
            int moduleId = d.u16(), ordinal = d.u16(), eventType = d.u16(), kind = d.u8(), monetary = d.u8();
            if (kind < 1 || kind > 3 || monetary > 1 || (monetary == 1 && kind != 2)) fail();
            effects.add(new ReceiptEffect(moduleId, ordinal, eventType, kind, monetary == 1,
                d.bounded(32), d.boundedAtMost(MAX_EFFECT_BODY)));
        }
        BigInteger feeCharged = d.integer(16); byte[] batchId = d.bounded(32); int moduleId = d.u16();
        long moduleVersion = d.u32(), parameterVersion = d.u32(); int operation = d.u8();
        byte[] asset = d.bounded(32); BigInteger amount = d.integer(16); byte[] from = d.bounded(32);
        BigInteger fromBefore = d.integer(16), fromAfter = d.integer(16), fromSequence = d.integer(8);
        byte[] to = d.bounded(32); BigInteger toBefore = d.integer(16), toAfter = d.integer(16);
        byte[] transferSetRoot = d.bounded(32), authorizationHash = d.bounded(32), contextHash = d.bounded(32);
        BigInteger timestamp = d.integer(8);
        if (globalSequence.signum() == 0) fail(ReceiptCheck.GLOBAL_SEQUENCE);
        if (moduleId == 0) fail(ReceiptCheck.MODULE_ID);
        if (moduleVersion == 0) fail(ReceiptCheck.MODULE_VERSION);
        if (timestamp.signum() == 0) fail(ReceiptCheck.TIMESTAMP);
        if (allZero(activityId)) fail(ReceiptCheck.ACTIVITY_ID);
        if (allZero(resultingStateRoot)) fail(ReceiptCheck.RESULTING_STATE_ROOT);
        ProgramReceiptOutcome programOutcome;
        try {
            programOutcome = d.remaining() > 69
                ? decodeProgramReceiptOutcomeFrom(d, protocolVersion) : null;
        } catch (PlatformSdkException error) {
            if (error.receiptCheck() != null) throw error;
            throw failure(ReceiptCheck.PROGRAM_OUTCOME);
        }
        if (programOutcome != null && (moduleId != GeneratedReceiptContract.PROGRAMS_MODULE_ID
                || programOutcome.resultCode() != resultCode
                || (programOutcome.terminalKind() == 1 && !equal(programOutcome.transferRoot(), transferSetRoot))
                || (programOutcome.terminalKind() != 1 && !allZero(transferSetRoot))))
            fail(ReceiptCheck.PROGRAM_OUTCOME);
        int signatureFlagOffset = d.position();
        if (d.u8() != 1) fail(ReceiptCheck.MISSING_SIGNATURE);
        byte[] signature = d.bounded(64); d.finish();
        ProtocolReceipt receipt = new ProtocolReceipt(protocolVersion, activityId, globalSequence,
            previousStateRoot, resultingStateRoot, activityRoot, resultCode, effects, feeCharged, batchId,
            moduleId, moduleVersion, parameterVersion, operation, asset, amount, from, fromBefore, fromAfter,
            fromSequence, to, toBefore, toAfter, transferSetRoot, authorizationHash, contextHash, timestamp,
            programOutcome, signature);
        byte[] unsigned = Arrays.copyOf(canonicalReceipt, signatureFlagOffset + 1);
        unsigned[signatureFlagOffset] = 0;
        return new DecodedReceipt(receipt, unsigned);
    }

    private static ProgramReceiptOutcome decodeProgramReceiptOutcomeFrom(Decoder d, int protocolVersion) {
        long tag = d.u32();
        int encodingVersion = tag == PROGRAM_OUTCOME_V1 ? 1
            : tag == PROGRAM_OUTCOME_V2 ? 2 : tag == PROGRAM_OUTCOME_V3 ? 3 : 0;
        if (encodingVersion == 0) fail();
        int terminalKind = d.u8(); int resultCode = d.i32(); int runtimeVersion = d.u16();
        int abiVersion = d.u16(); long feeScheduleVersion = d.u32();
        long meteringScheduleVersion = encodingVersion == 3 ? d.u32() : 1;
        BigInteger cpuFuel = d.integer(8), memoryBytes = d.integer(8);
        BigInteger storageReadBytes = d.integer(8), storageWriteBytes = d.integer(8);
        long outputValues = d.u32(); BigInteger outputBytes = d.integer(8);
        BigInteger occupancyByteBatches = BigInteger.ZERO, occupancyFeeUnits = BigInteger.ZERO;
        List<BigInteger> feeSchedulePrices = new ArrayList<>(7);
        byte[] occupancyAssetId = new byte[32], occupancyEvidenceDigest = new byte[32];
        byte[] occupancyTransferRoot = new byte[32];
        if (encodingVersion >= 2) {
            occupancyByteBatches = d.integer(16); occupancyFeeUnits = d.integer(16);
            for (int index = 0; index < 7; index++) feeSchedulePrices.add(d.integer(8));
            occupancyAssetId = d.bounded(32); occupancyEvidenceDigest = d.bounded(32);
            occupancyTransferRoot = d.bounded(32);
        } else {
            for (int index = 0; index < 7; index++) feeSchedulePrices.add(BigInteger.ZERO);
        }
        BigInteger feeUnits = d.integer(16); byte[] callGraphRoot = d.bounded(32);
        byte[] terminalPayloadRoot = d.bounded(32); byte[] transferRoot = d.bounded(32);
        boolean occupancyZero = occupancyByteBatches.signum() == 0 && occupancyFeeUnits.signum() == 0
            && allZero(occupancyAssetId) && allZero(occupancyEvidenceDigest) && allZero(occupancyTransferRoot);
        boolean validVersion = protocolVersion == 1 && (encodingVersion == 1 || encodingVersion == 3)
            || protocolVersion == 2 && (encodingVersion == 2 || encodingVersion == 3);
        if (terminalKind < 1 || terminalKind > 3 || runtimeVersion == 0 || abiVersion == 0
                || feeScheduleVersion == 0 || meteringScheduleVersion != 1 || allZero(terminalPayloadRoot)
                || terminalKind == 1 && resultCode != 0
                || terminalKind != 1 && (resultCode == 0 || resultCode <= -1000)
                || terminalKind != 1 && !allZero(transferRoot) || !validVersion
                || encodingVersion == 1 && !occupancyZero
                || encodingVersion >= 2 && terminalKind != 1 && !occupancyZero
                || encodingVersion == 2 && terminalKind == 1
                    && (allZero(occupancyAssetId) || allZero(occupancyEvidenceDigest))
                || encodingVersion == 3 && allZero(occupancyAssetId) != allZero(occupancyEvidenceDigest)
                || protocolVersion == 1 && encodingVersion == 3 && !occupancyZero
                || protocolVersion == 2 && encodingVersion == 3 && terminalKind == 1
                    && (allZero(occupancyAssetId) || allZero(occupancyEvidenceDigest))) fail();
        return new ProgramReceiptOutcome(encodingVersion, terminalKind, resultCode, runtimeVersion,
            abiVersion, feeScheduleVersion, meteringScheduleVersion, cpuFuel, memoryBytes,
            storageReadBytes, storageWriteBytes, outputValues, outputBytes, occupancyByteBatches,
            occupancyFeeUnits, feeSchedulePrices, occupancyAssetId, occupancyEvidenceDigest,
            occupancyTransferRoot, feeUnits, callGraphRoot, terminalPayloadRoot, transferRoot);
    }

    public static ProgramReceiptOutcome decodeProgramReceiptOutcome(byte[] canonicalOutcome,
                                                                     int protocolVersion) {
        if (canonicalOutcome == null || canonicalOutcome.length == 0
                || canonicalOutcome.length > MAX_MESSAGE_BYTES) fail(ReceiptCheck.RECEIPT_SHAPE);
        try {
            Decoder decoder = new Decoder(canonicalOutcome.clone());
            ProgramReceiptOutcome outcome = decodeProgramReceiptOutcomeFrom(decoder, protocolVersion);
            decoder.finish();
            return outcome;
        } catch (PlatformSdkException error) {
            if (error.receiptCheck() != null) throw error;
            throw failure(ReceiptCheck.PROGRAM_OUTCOME);
        }
    }

    private static byte[] attestationMessage(CheckpointAttestation a) {
        return concat(unsigned(BigInteger.valueOf(a.protocolVersion()), 2),
            unsigned(BigInteger.valueOf(a.networkId()), 4), unsigned(a.paxeerChainId(), 8),
            exact(a.settlementContract(), 20), unsigned(a.epoch(), 8),
            exact(a.checkpointId(), 32), exact(a.checkpointHash(), 32), exact(a.guarantorId(), 32),
            unsigned(a.batchNumber(), 8), exact(a.dataAvailabilityRoot(), 32),
            new byte[]{(byte) (a.replayed() ? 1 : 0), (byte) (a.dataPossessed() ? 1 : 0),
                (byte) a.availabilityClassMask()}, unsigned(a.attestedAtMs(), 8));
    }
    private static int proofDepth(long count) { int depth = 0; while (count > 1) { count = (count + 1) / 2; depth++; } return depth; }
    private static void field(Decoder decoder, int expected) { if (decoder.u8() != expected) fail(); }
    private static boolean allZero(byte[] value) { int aggregate = 0; for (byte item : value) aggregate |= item; return aggregate == 0; }
    private static boolean equal(byte[] left, byte[] right) { return MessageDigest.isEqual(left, right); }
    private static byte[] exact(byte[] value, int length) { if (value == null || value.length != length) fail(); return value; }
    private static byte[] u32(int value) { return ByteBuffer.allocate(4).putInt(value).array(); }
    private static byte[] unsigned(BigInteger value, int length) {
        if (value == null || value.signum() < 0 || value.bitLength() > length * 8) fail();
        byte[] raw = value.toByteArray(), out = new byte[length];
        int copy = Math.min(raw.length, length);
        System.arraycopy(raw, raw.length - copy, out, length - copy, copy);
        return out;
    }
    private static byte[] sha256(byte[]... values) {
        try {
            MessageDigest digest = MessageDigest.getInstance("SHA-256");
            for (byte[] value : values) digest.update(value);
            return digest.digest();
        } catch (java.security.GeneralSecurityException impossible) { throw new AssertionError(impossible); }
    }
    private static boolean verifyEd25519(byte[] publicKey, byte[] signature, byte[] digest) {
        try {
            byte[] encoded = concat(ED25519_X509_PREFIX, exact(publicKey, 32));
            var key = KeyFactory.getInstance("Ed25519").generatePublic(new X509EncodedKeySpec(encoded));
            var verifier = Signature.getInstance("Ed25519"); verifier.initVerify(key); verifier.update(exact(digest, 32));
            return verifier.verify(exact(signature, 64));
        } catch (java.security.GeneralSecurityException | RuntimeException error) { return false; }
    }
    private static byte[] concat(byte[]... values) {
        var output = new ByteArrayOutputStream();
        for (byte[] value : values) output.writeBytes(value);
        return output.toByteArray();
    }
    private static byte[] ascii(String value) { return value.getBytes(StandardCharsets.UTF_8); }
    private static byte[] hex(String value) { return java.util.HexFormat.of().parseHex(value); }
    private static PlatformSdkException failure() { return PlatformSdkException.verificationFailure(); }
    private static PlatformSdkException failure(ReceiptCheck check) {
        return PlatformSdkException.receiptVerification(check);
    }
    private static void fail() { throw failure(); }
    private static void fail(ReceiptCheck check) { throw failure(check); }

    private static final class Decoder {
        private final byte[] bytes; private int offset;
        private Decoder(byte[] bytes) { this.bytes = bytes; }
        int u8() { return fixed(1)[0] & 0xff; }
        int u16() { return integer(2).intValueExact(); }
        long u32() { return integer(4).longValueExact(); }
        int i32() { long value = u32(); return value > 0x7fff_ffffL ? (int) (value - 0x1_0000_0000L) : (int) value; }
        int position() { return offset; }
        int remaining() { return bytes.length - offset; }
        byte[] fixed(int length) {
            if (length < 0 || offset > bytes.length - length) fail();
            byte[] value = Arrays.copyOfRange(bytes, offset, offset + length); offset += length; return value;
        }
        byte[] bounded(int length) { if (u32() != length) fail(); return fixed(length); }
        byte[] boundedAtMost(int maximum) { long length = u32(); if (length > maximum) fail(); return fixed((int) length); }
        BigInteger integer(int length) { return new BigInteger(1, fixed(length)); }
        void finish() { if (offset != bytes.length) fail(); }
    }
}
