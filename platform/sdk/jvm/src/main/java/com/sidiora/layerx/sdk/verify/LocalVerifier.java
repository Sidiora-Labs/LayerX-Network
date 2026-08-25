package com.sidiora.layerx.sdk.verify;

import com.sidiora.layerx.sdk.PlatformSdkException;
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
import org.bouncycastle.crypto.params.ECDomainParameters;
import org.bouncycastle.crypto.params.ECPublicKeyParameters;
import org.bouncycastle.crypto.signers.ECDSASigner;

/** Trustless 402LXP receipt, Merkle, batch, and checkpoint verification. */
public final class LocalVerifier {
    private LocalVerifier() {}

    private static final byte[] MERKLE_LEAF_DOMAIN = ascii("LXP/v1/merkle-leaf\0");
    private static final byte[] MERKLE_INTERNAL_DOMAIN = ascii("LXP/v1/merkle-internal\0");
    private static final byte[] BATCH_HEADER_DOMAIN = ascii("LXP/v1/batch-header\0");
    private static final byte[] RECEIPT_DOMAIN = ascii("LXP/v1/receipt\0");
    private static final byte[] CHECKPOINT_DOMAIN = ascii("LXP/v1/checkpoint-certificate\0");
    private static final byte[] GUARANTOR_ATTESTATION_DOMAIN = ascii("LXP/v1/guarantor-attestation\0");
    private static final byte[] ED25519_X509_PREFIX = hex("302a300506032b6570032100");
    private static final int BATCH_HEADER_BYTES = 354;
    private static final int MAX_MESSAGE_BYTES = 1_048_576;
    private static final int MAX_EFFECTS = 512;
    private static final int MAX_EFFECT_BODY = 256;
    private static final int ALL_AVAILABILITY_CLASSES = 0x1f;
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
        boolean dataPossessed, int availabilityClassMask, BigInteger attestedAtMs, byte[] signature) {}
    public record GuarantorKey(byte[] guarantorId, byte[] publicKey, boolean bonded) {}
    public record CheckpointCertificate(byte[] canonicalHeader, byte[] validityProof,
        List<CheckpointAttestation> attestations, int threshold, byte[] settlementReference) {
        public CheckpointCertificate { attestations = List.copyOf(attestations); }
    }
    public record CheckpointVerificationInput(CheckpointCertificate certificate, List<GuarantorKey> bondedSet,
        byte[] registeredCheckpointId, byte[] registeredSettlementReference, boolean availabilityObtained) {
        public CheckpointVerificationInput { bondedSet = List.copyOf(bondedSet); }
    }
    public record CheckpointVerification(VerificationLevel level, byte[] checkpointId, int achieved,
        int required, BatchHeader header) {}
    @FunctionalInterface public interface LocalSignatureVerifier {
        boolean verifySecp256k1(byte[] publicKey, byte[] signature, byte[] digest);
    }
    public record ReceiptEffect(int moduleId, int ordinal, int eventType, int kind, boolean monetary,
        byte[] transferSetRoot, byte[] body) {}
    public record ProtocolReceipt(int protocolVersion, byte[] activityId, BigInteger globalSequence,
        byte[] previousStateRoot, byte[] resultingStateRoot, byte[] activityRoot, int resultCode,
        List<ReceiptEffect> effects, BigInteger feeCharged, byte[] batchId, int moduleId,
        long moduleVersion, long parameterVersion, int operation, byte[] asset, BigInteger amount,
        byte[] from, BigInteger fromBalanceBefore, BigInteger fromBalanceAfter, BigInteger fromSequence,
        byte[] to, BigInteger toBalanceBefore, BigInteger toBalanceAfter, byte[] transferSetRoot,
        byte[] authorizationHash, byte[] contextHash, BigInteger timestamp, byte[] sequencerSignature) {
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
        byte[] checkpointId = sha256(CHECKPOINT_DOMAIN, certificate.canonicalHeader(),
            u32(certificate.validityProof().length), certificate.validityProof());
        if (!equal(checkpointId, exact(input.registeredCheckpointId(), 32)) || certificate.threshold() <= 0) fail();
        Set<String> seen = new HashSet<>();
        int achieved = 0;
        BigInteger paxeerChainId = null;
        byte[] settlementContract = null;
        for (CheckpointAttestation attestation : certificate.attestations()) {
            byte[] guarantorId = exact(attestation.guarantorId(), 32);
            byte[] attestationSettlementContract = exact(attestation.settlementContract(), 20);
            if (!seen.add(java.util.HexFormat.of().formatHex(guarantorId))
                    || attestation.protocolVersion() != header.protocolVersion()
                    || attestation.networkId() != header.networkId()
                    || !attestation.epoch().equals(header.epoch())
                    || attestation.paxeerChainId().signum() <= 0
                    || allZero(attestationSettlementContract)
                    || (paxeerChainId != null && (!attestation.paxeerChainId().equals(paxeerChainId)
                        || !equal(attestationSettlementContract, settlementContract)))
                    || !equal(attestation.checkpointId(), checkpointId)
                    || !equal(attestation.checkpointHash(), checkpointId)
                    || !attestation.batchNumber().equals(header.batchNumber())
                    || !equal(attestation.dataAvailabilityRoot(), header.dataAvailabilityRoot())
                    || !attestation.replayed() || !attestation.dataPossessed()
                    || attestation.availabilityClassMask() != ALL_AVAILABILITY_CLASSES
                    || attestation.attestedAtMs().signum() <= 0) fail();
            paxeerChainId = attestation.paxeerChainId();
            settlementContract = attestationSettlementContract;
            GuarantorKey member = input.bondedSet().stream().filter(candidate -> candidate.bonded()
                && equal(candidate.guarantorId(), guarantorId)).findFirst().orElseThrow(LocalVerifier::failure);
            byte[] digest = sha256(GUARANTOR_ATTESTATION_DOMAIN, attestationMessage(attestation));
            if (!signatures.verifySecp256k1(exact(member.publicKey(), 33),
                    exact(attestation.signature(), 64), digest)) fail();
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
        return verifyCheckpoint(input, LocalVerifier::verifySecp256k1);
    }

    /** Production raw compact secp256k1 verifier for checkpoint attestations. */
    public static boolean verifySecp256k1(byte[] publicKey, byte[] signature, byte[] digest) {
        try {
            exact(publicKey, 33); exact(signature, 64); exact(digest, 32);
            var params = CustomNamedCurves.getByName("secp256k1");
            var domain = new ECDomainParameters(params.getCurve(), params.getG(), params.getN(), params.getH());
            var key = new ECPublicKeyParameters(params.getCurve().decodePoint(publicKey), domain);
            var verifier = new ECDSASigner();
            verifier.init(false, key);
            return verifier.verifySignature(digest, new BigInteger(1, Arrays.copyOfRange(signature, 0, 32)),
                new BigInteger(1, Arrays.copyOfRange(signature, 32, 64)));
        } catch (RuntimeException error) {
            return false;
        }
    }

    public static ReceiptVerification verifyReceiptOutcome(byte[] canonicalReceipt,
                                                             AuthorizedReceiptBatch authorized) {
        DecodedReceipt decoded = decodeProtocolReceipt(canonicalReceipt);
        ProtocolReceipt receipt = decoded.receipt();
        if (receipt.operation() == 0 || allZero(receipt.activityId()) || allZero(receipt.asset())
                || !equal(receipt.batchId(), exact(authorized.batchId(), 32))
                || !equal(receipt.asset(), exact(authorized.asset(), 32))
                || !equal(receipt.previousStateRoot(), exact(authorized.previousStateRoot(), 32))
                || !equal(receipt.resultingStateRoot(), exact(authorized.resultingStateRoot(), 32))) fail();
        if (receipt.resultCode() == 0) {
            BigInteger expectedFrom = receipt.fromBalanceBefore().subtract(receipt.amount());
            BigInteger expectedTo = receipt.toBalanceBefore().add(receipt.amount());
            if (receipt.fromBalanceBefore().compareTo(receipt.amount()) < 0
                    || !expectedFrom.equals(receipt.fromBalanceAfter())
                    || expectedTo.compareTo(MAX_U128) > 0 || !expectedTo.equals(receipt.toBalanceAfter())) fail();
        }
        byte[] digest = sha256(RECEIPT_DOMAIN, decoded.unsignedBytes());
        if (!verifyEd25519(authorized.sequencerPublicKey(), receipt.sequencerSignature(), digest)) fail();
        return new ReceiptVerification(VerificationLevel.SEQUENCER_SIGNED, receipt,
            canonicalReceipt.clone(), digest);
    }

    public static ReceiptVerification verifyReceipt(byte[] canonicalReceipt, AuthorizedReceiptBatch authorized) {
        ReceiptVerification verified = verifyReceiptOutcome(canonicalReceipt, authorized);
        if (verified.receipt().resultCode() != 0) fail();
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
        if (canonicalReceipt == null || canonicalReceipt.length == 0 || canonicalReceipt.length > MAX_MESSAGE_BYTES) fail();
        Decoder d = new Decoder(canonicalReceipt);
        if (d.u16() != 1 || d.u16() != 0x5201) fail();
        int protocolVersion = d.u16(); if (protocolVersion != 1) fail();
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
        BigInteger timestamp = d.integer(8); int signatureFlagOffset = d.position();
        if (d.u8() != 1) fail();
        byte[] signature = d.bounded(64); d.finish();
        ProtocolReceipt receipt = new ProtocolReceipt(protocolVersion, activityId, globalSequence,
            previousStateRoot, resultingStateRoot, activityRoot, resultCode, effects, feeCharged, batchId,
            moduleId, moduleVersion, parameterVersion, operation, asset, amount, from, fromBefore, fromAfter,
            fromSequence, to, toBefore, toAfter, transferSetRoot, authorizationHash, contextHash, timestamp, signature);
        byte[] unsigned = Arrays.copyOf(canonicalReceipt, signatureFlagOffset + 1);
        unsigned[signatureFlagOffset] = 0;
        return new DecodedReceipt(receipt, unsigned);
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
    private static void fail() { throw failure(); }

    private static final class Decoder {
        private final byte[] bytes; private int offset;
        private Decoder(byte[] bytes) { this.bytes = bytes; }
        int u8() { return fixed(1)[0] & 0xff; }
        int u16() { return integer(2).intValueExact(); }
        long u32() { return integer(4).longValueExact(); }
        int i32() { long value = u32(); return value > 0x7fff_ffffL ? (int) (value - 0x1_0000_0000L) : (int) value; }
        int position() { return offset; }
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
