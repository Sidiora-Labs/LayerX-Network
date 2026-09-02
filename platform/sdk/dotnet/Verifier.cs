#nullable enable

using System.Buffers.Binary;
using System.Security.Cryptography;
using System.Text;
using Org.BouncyCastle.Asn1.Sec;
using Org.BouncyCastle.Crypto.Parameters;
using Org.BouncyCastle.Crypto.Digests;
using Org.BouncyCastle.Crypto.Signers;
using Org.BouncyCastle.Math;
using Org.BouncyCastle.Math.EC;

namespace LayerX.Sdk;

public readonly record struct UInt128Value(ulong High, ulong Low)
{
    internal bool TryAdd(UInt128Value other, out UInt128Value result)
    {
        var low = unchecked(Low + other.Low);
        var carry = low < Low ? 1UL : 0UL;
        var high = unchecked(High + other.High);
        if (high < High || ulong.MaxValue - high < carry) { result = default; return false; }
        result = new(high + carry, low);
        return true;
    }

    internal bool TrySubtract(UInt128Value other, out UInt128Value result)
    {
        var borrow = Low < other.Low ? 1UL : 0UL;
        if (High < other.High || High - other.High < borrow) { result = default; return false; }
        result = new(High - other.High - borrow, unchecked(Low - other.Low));
        return true;
    }
}

public sealed record MerkleProof(uint LeafIndex, uint LeafCount, IReadOnlyList<byte[]> Siblings);

public sealed record BatchHeader(
    ushort ProtocolVersion,
    uint NetworkId,
    ulong Epoch,
    ulong BatchNumber,
    ulong FirstSequence,
    ulong LastSequence,
    byte[] PreviousStateRoot,
    byte[] ResultingStateRoot,
    byte[] ActivityMerkleRoot,
    byte[] ReceiptMerkleRoot,
    byte[] EventMerkleRoot,
    byte[] DataAvailabilityRoot,
    byte[] OracleRoot,
    ulong TimestampMilliseconds,
    byte[] SequencerId);

public sealed record SequencerAuthorization(byte[] SequencerId, byte[] PublicKey, ulong FirstBatchNumber, ulong LastBatchNumber);
public enum InclusionKind { Activity, Receipt, Event, State }
public sealed record InclusionVerification(string Level, BatchHeader Header, byte[] HeaderDigest, byte[] Root);

public sealed record CheckpointAttestation(
    ushort ProtocolVersion,
    uint NetworkId,
    ulong PaxeerChainId,
    byte[] SettlementContract,
    ulong Epoch,
    byte[] CheckpointId,
    byte[] CheckpointHash,
    byte[] GuarantorId,
    ulong BatchNumber,
    byte[] DataAvailabilityRoot,
    bool Replayed,
    bool DataPossessed,
    byte AvailabilityClassMask,
    ulong AttestedAtMilliseconds,
    byte[] Signer,
    byte[] Signature,
    byte SignatureV);

public sealed record GuarantorKey(byte[] GuarantorId, byte[] PublicKey, bool Bonded);
public sealed record CheckpointCertificate(byte[] CanonicalHeader, byte[] ValidityProof, IReadOnlyList<CheckpointAttestation> Attestations, uint Threshold, byte[]? SettlementReference = null);
public sealed record CheckpointVerificationInput(CheckpointCertificate Certificate, IReadOnlyList<GuarantorKey> BondedSet, byte[] RegisteredCheckpointId, ulong ExpectedPaxeerChainId, byte[] ExpectedSettlementContract, byte[]? RegisteredSettlementReference, bool AvailabilityObtained);
public sealed record CheckpointVerification(string Level, byte[] CheckpointId, uint Achieved, uint Required, BatchHeader Header);

public interface ILocalSignatureVerifier
{
    ValueTask<bool> VerifyRecoverableSecp256k1Async(ReadOnlyMemory<byte> publicKey, ReadOnlyMemory<byte> signature, byte signatureV, ReadOnlyMemory<byte> signer, ReadOnlyMemory<byte> digest, CancellationToken cancellationToken = default);
}

public sealed class BouncyCastleSignatureVerifier : ILocalSignatureVerifier
{
    public ValueTask<bool> VerifyRecoverableSecp256k1Async(ReadOnlyMemory<byte> publicKey, ReadOnlyMemory<byte> signature, byte signatureV, ReadOnlyMemory<byte> signer, ReadOnlyMemory<byte> digest, CancellationToken cancellationToken = default)
    {
        cancellationToken.ThrowIfCancellationRequested();
        if (publicKey.Length != 33 || signature.Length != 64 || signer.Length != 20 || digest.Length != 32 || (signatureV != 27 && signatureV != 28)) return ValueTask.FromResult(false);
        try
        {
            var curve = SecNamedCurves.GetByName("secp256k1");
            var point = curve.Curve.DecodePoint(publicKey.ToArray());
            var domain = new ECDomainParameters(curve.Curve, curve.G, curve.N, curve.H, curve.GetSeed());
            var key = new ECPublicKeyParameters(point, domain);
            var encodedSignature = signature.Span;
            var r = new BigInteger(1, encodedSignature[..32].ToArray());
            var s = new BigInteger(1, encodedSignature[32..].ToArray());
            if (r.SignValue <= 0 || r.CompareTo(curve.N) >= 0 || s.SignValue <= 0 || s.CompareTo(curve.N.ShiftRight(1)) > 0)
                return ValueTask.FromResult(false);
            var verifier = new ECDsaSigner();
            verifier.Init(false, key);
            if (!verifier.VerifySignature(digest.ToArray(), r, s)) return ValueTask.FromResult(false);
            var compressed = new byte[33];
            compressed[0] = (byte)(2 + signatureV - 27);
            var x = r.ToByteArrayUnsigned();
            if (x.Length > 32) return ValueTask.FromResult(false);
            x.CopyTo(compressed, compressed.Length - x.Length);
            var recoveredR = curve.Curve.DecodePoint(compressed);
            if (!recoveredR.Multiply(curve.N).IsInfinity) return ValueTask.FromResult(false);
            var inverseR = r.ModInverse(curve.N);
            var e = new BigInteger(1, digest.ToArray());
            var recovered = ECAlgorithms.SumOfTwoMultiplies(
                curve.G, e.Negate().Mod(curve.N).Multiply(inverseR).Mod(curve.N),
                recoveredR, s.Multiply(inverseR).Mod(curve.N)).Normalize();
            if (!CryptographicOperations.FixedTimeEquals(recovered.GetEncoded(true), publicKey.Span))
                return ValueTask.FromResult(false);
            var uncompressed = recovered.GetEncoded(false);
            var keccak = new KeccakDigest(256);
            keccak.BlockUpdate(uncompressed, 1, uncompressed.Length - 1);
            var addressHash = new byte[32];
            keccak.DoFinal(addressHash, 0);
            return ValueTask.FromResult(CryptographicOperations.FixedTimeEquals(addressHash.AsSpan(12, 20), signer.Span));
        }
        catch (Exception exception) when (exception is ArgumentException or InvalidOperationException or ArithmeticException)
        {
            return ValueTask.FromResult(false);
        }
    }
}

public sealed record ReceiptEffect(ushort ModuleId, ushort Ordinal, ushort EventType, byte Kind, bool Monetary, byte[] TransferSetRoot, byte[] Body);

public sealed record ProgramReceiptOutcome(
    byte EncodingVersion,
    byte TerminalKind,
    int ResultCode,
    ushort RuntimeVersion,
    ushort AbiVersion,
    uint FeeScheduleVersion,
    uint MeteringScheduleVersion,
    ulong CpuFuel,
    ulong MemoryBytes,
    ulong StorageReadBytes,
    ulong StorageWriteBytes,
    uint OutputValues,
    ulong OutputBytes,
    UInt128Value OccupancyByteBatches,
    UInt128Value OccupancyFeeUnits,
    IReadOnlyList<ulong> FeeSchedulePrices,
    byte[] OccupancyAssetId,
    byte[] OccupancyEvidenceDigest,
    byte[] OccupancyTransferRoot,
    UInt128Value FeeUnits,
    byte[] CallGraphRoot,
    byte[] TerminalPayloadRoot,
    byte[] TransferRoot);

public sealed record ProtocolReceipt(
    ushort ProtocolVersion,
    byte[] ActivityId,
    ulong GlobalSequence,
    byte[] PreviousStateRoot,
    byte[] ResultingStateRoot,
    byte[] ActivityRoot,
    int ResultCode,
    IReadOnlyList<ReceiptEffect> Effects,
    UInt128Value FeeCharged,
    byte[] BatchId,
    ushort ModuleId,
    uint ModuleVersion,
    uint ParameterVersion,
    byte Operation,
    byte[] Asset,
    UInt128Value Amount,
    byte[] From,
    UInt128Value FromBalanceBefore,
    UInt128Value FromBalanceAfter,
    ulong FromSequence,
    byte[] To,
    UInt128Value ToBalanceBefore,
    UInt128Value ToBalanceAfter,
    byte[] TransferSetRoot,
    byte[] AuthorizationHash,
    byte[] ContextHash,
    ulong Timestamp,
    ProgramReceiptOutcome? ProgramOutcome,
    byte[] SequencerSignature);

public sealed record AuthorizedReceiptBatch(byte[] BatchId, byte[] Asset, byte[] PreviousStateRoot, byte[] ResultingStateRoot, byte[] SequencerPublicKey);
public sealed record ReceiptVerification(string Level, ProtocolReceipt Receipt, byte[] CanonicalBytes, byte[] ReceiptDigest);

public static class LocalVerifier
{
    public static bool VerifyEd25519Digest(ReadOnlySpan<byte> publicKey, ReadOnlySpan<byte> signature, ReadOnlySpan<byte> digest) =>
        VerifyEd25519(Exact(publicKey.ToArray(), 32), Exact(signature.ToArray(), 64), Exact(digest.ToArray(), 32));

    private static readonly byte[] MerkleLeafDomain = Encoding.UTF8.GetBytes("LXP/v1/merkle-leaf\0");
    private static readonly byte[] MerkleInternalDomain = Encoding.UTF8.GetBytes("LXP/v1/merkle-internal\0");
    private static readonly byte[] BatchHeaderDomain = Encoding.UTF8.GetBytes("LXP/v1/batch-header\0");
    private static readonly byte[] ReceiptDomain = Encoding.UTF8.GetBytes("LXP/v1/receipt\0");
    private static readonly byte[] CheckpointDomain = Encoding.UTF8.GetBytes("LXP/v2/checkpoint-certificate\0");
    private static readonly byte[] GuarantorAttestationDomain = Encoding.UTF8.GetBytes("LXP/v2/guarantor-attestation\0");
    private const int MaximumMessageBytes = 1_048_576;
    private const uint MaximumEffects = 512;
    private const uint MaximumEffectBody = 256;
    private const int BatchHeaderBytes = 354;
    private const byte AllAvailabilityClasses = 0x1f;
    private const ushort CurrentProtocolVersion = 2;
    private const uint ProgramOutcomeV1 = GeneratedReceiptContract.ProgramOutcomeV1;
    private const uint ProgramOutcomeV2 = GeneratedReceiptContract.ProgramOutcomeV2;
    private const uint ProgramOutcomeV3 = GeneratedReceiptContract.ProgramOutcomeV3;

    public static void VerifyMerkleInclusion(ReadOnlySpan<byte> canonicalLeaf, MerkleProof proof, ReadOnlySpan<byte> expectedRoot)
    {
        ArgumentNullException.ThrowIfNull(proof);
        if (proof.LeafCount == 0 || proof.LeafIndex >= proof.LeafCount || proof.Siblings is null || proof.Siblings.Count > 32 || proof.Siblings.Count != ProofDepth(proof.LeafCount) || expectedRoot.Length != 32)
            throw VerificationFailure();
        var current = Digest(MerkleLeafDomain, canonicalLeaf.ToArray());
        var index = proof.LeafIndex;
        var count = proof.LeafCount;
        foreach (var untrustedSibling in proof.Siblings)
        {
            var sibling = Exact(untrustedSibling, 32);
            if ((index ^ 1) >= count && !Equal(sibling, current)) throw VerificationFailure();
            current = index % 2 == 0
                ? Digest(MerkleInternalDomain, current, sibling)
                : Digest(MerkleInternalDomain, sibling, current);
            index /= 2;
            count = count / 2 + count % 2;
        }
        if (!Equal(current, expectedRoot)) throw VerificationFailure();
    }

    public static BatchHeader DecodeBatchHeader(ReadOnlySpan<byte> canonicalHeader)
    {
        if (canonicalHeader.Length != BatchHeaderBytes) throw VerificationFailure();
        var decoder = new WireDecoder(canonicalHeader.ToArray());
        if (decoder.U16() != 1 || decoder.U16() != 0x1701 || decoder.U8() != 15) throw VerificationFailure();
        void Field(byte expected) { if (decoder.U8() != expected) throw VerificationFailure(); }
        Field(1); var protocolVersion = decoder.U16();
        Field(2); var networkId = decoder.U32();
        Field(3); var epoch = decoder.U64();
        Field(4); var batchNumber = decoder.U64();
        Field(5); var firstSequence = decoder.U64();
        Field(6); var lastSequence = decoder.U64();
        Field(7); var previousStateRoot = decoder.Array32();
        Field(8); var resultingStateRoot = decoder.Array32();
        Field(9); var activityMerkleRoot = decoder.Array32();
        Field(10); var receiptMerkleRoot = decoder.Array32();
        Field(11); var eventMerkleRoot = decoder.Array32();
        Field(12); var dataAvailabilityRoot = decoder.Array32();
        Field(13); var oracleRoot = decoder.Array32();
        Field(14); var timestampMilliseconds = decoder.U64();
        Field(15); var sequencerId = decoder.Array32();
        decoder.Finish();
        return new(protocolVersion, networkId, epoch, batchNumber, firstSequence, lastSequence, previousStateRoot, resultingStateRoot, activityMerkleRoot, receiptMerkleRoot, eventMerkleRoot, dataAvailabilityRoot, oracleRoot, timestampMilliseconds, sequencerId);
    }

    public static ValueTask<InclusionVerification> VerifyBatchInclusionAsync(InclusionKind kind, ReadOnlyMemory<byte> canonicalLeaf, MerkleProof proof, ReadOnlyMemory<byte> canonicalHeader, ReadOnlyMemory<byte> headerSignature, SequencerAuthorization authorization, CancellationToken cancellationToken = default)
    {
        cancellationToken.ThrowIfCancellationRequested();
        var header = DecodeBatchHeader(canonicalHeader.Span);
        if (header.ProtocolVersion != CurrentProtocolVersion) throw VerificationFailure();
        if (header.BatchNumber < authorization.FirstBatchNumber || header.BatchNumber > authorization.LastBatchNumber || !Equal(header.SequencerId, Exact(authorization.SequencerId, 32)))
            throw VerificationFailure();
        var headerDigest = Digest(BatchHeaderDomain, canonicalHeader.ToArray());
        if (!VerifyEd25519(authorization.PublicKey, headerSignature.Span, headerDigest)) throw VerificationFailure();
        var root = kind switch
        {
            InclusionKind.Activity => header.ActivityMerkleRoot,
            InclusionKind.Receipt => header.ReceiptMerkleRoot,
            InclusionKind.Event => header.EventMerkleRoot,
            InclusionKind.State => header.ResultingStateRoot,
            _ => throw VerificationFailure(),
        };
        VerifyMerkleInclusion(canonicalLeaf.Span, proof, root);
        return ValueTask.FromResult(new InclusionVerification(kind == InclusionKind.State ? "state-proven" : "batch-included", header, headerDigest, root.ToArray()));
    }

    public static async ValueTask<CheckpointVerification> VerifyCheckpointAsync(CheckpointVerificationInput input, ILocalSignatureVerifier signatures, CancellationToken cancellationToken = default)
    {
        ArgumentNullException.ThrowIfNull(input);
        ArgumentNullException.ThrowIfNull(signatures);
        var certificate = input.Certificate ?? throw VerificationFailure();
        if (!input.AvailabilityObtained || certificate.Threshold == 0 || certificate.ValidityProof is null || certificate.Attestations is null || certificate.ValidityProof.LongLength > uint.MaxValue)
            throw VerificationFailure();
        var header = DecodeBatchHeader(certificate.CanonicalHeader);
        if (header.ProtocolVersion != CurrentProtocolVersion) throw VerificationFailure();
        var checkpointId = Digest(CheckpointDomain, certificate.CanonicalHeader, EncodeUInt32((uint)certificate.ValidityProof.Length), certificate.ValidityProof);
        var expectedSettlementContract = Exact(input.ExpectedSettlementContract, 20);
        if (!Equal(checkpointId, Exact(input.RegisteredCheckpointId, 32)) || input.ExpectedPaxeerChainId == 0 || AllZero(expectedSettlementContract)) throw VerificationFailure();
        var bonded = new Dictionary<string, GuarantorKey>(StringComparer.Ordinal);
        foreach (var member in input.BondedSet ?? throw VerificationFailure())
            if (member.Bonded) bonded[Convert.ToHexString(Exact(member.GuarantorId, 32))] = member;
        var seen = new HashSet<string>(StringComparer.Ordinal);
        uint achieved = 0;
        foreach (var attestation in certificate.Attestations)
        {
            cancellationToken.ThrowIfCancellationRequested();
            var guarantorId = Exact(attestation.GuarantorId, 32);
            var attestationSettlementContract = Exact(attestation.SettlementContract, 20);
            var identity = Convert.ToHexString(guarantorId);
            if (!seen.Add(identity) || attestation.ProtocolVersion != header.ProtocolVersion || attestation.NetworkId != header.NetworkId || attestation.Epoch != header.Epoch ||
                attestation.PaxeerChainId != input.ExpectedPaxeerChainId || !Equal(attestationSettlementContract, expectedSettlementContract) ||
                !Equal(Exact(attestation.CheckpointId, 32), checkpointId) || !Equal(Exact(attestation.CheckpointHash, 32), checkpointId) ||
                attestation.BatchNumber != header.BatchNumber || !Equal(Exact(attestation.DataAvailabilityRoot, 32), header.DataAvailabilityRoot) ||
                !attestation.Replayed || !attestation.DataPossessed || attestation.AvailabilityClassMask != AllAvailabilityClasses || attestation.AttestedAtMilliseconds == 0 ||
                AllZero(Exact(attestation.Signer, 20)) || (attestation.SignatureV != 27 && attestation.SignatureV != 28) ||
                !bonded.TryGetValue(identity, out var member) || achieved == uint.MaxValue)
                throw VerificationFailure();
            var message = Concatenate(
                EncodeUInt16(attestation.ProtocolVersion), EncodeUInt32(attestation.NetworkId), EncodeUInt64(attestation.PaxeerChainId),
                attestationSettlementContract, EncodeUInt64(attestation.Epoch),
                Exact(attestation.CheckpointId, 32), Exact(attestation.CheckpointHash, 32), guarantorId,
                EncodeUInt64(attestation.BatchNumber), Exact(attestation.DataAvailabilityRoot, 32),
                new byte[] { 1, 1, attestation.AvailabilityClassMask }, EncodeUInt64(attestation.AttestedAtMilliseconds));
            var attestationDigest = Digest(GuarantorAttestationDomain, message);
            if (!await signatures.VerifyRecoverableSecp256k1Async(Exact(member.PublicKey, 33), Exact(attestation.Signature, 64), attestation.SignatureV, Exact(attestation.Signer, 20), attestationDigest, cancellationToken).ConfigureAwait(false))
                throw VerificationFailure();
            achieved++;
        }
        if (achieved < certificate.Threshold) throw VerificationFailure();
        var level = "checkpoint-finalised";
        if (certificate.SettlementReference is not null)
        {
            if (certificate.SettlementReference.Length == 0 || input.RegisteredSettlementReference is null || !Equal(certificate.SettlementReference, input.RegisteredSettlementReference))
                throw VerificationFailure();
            level = "settlement-anchored";
        }
        return new(level, checkpointId, achieved, certificate.Threshold, header);
    }

    public static ValueTask<ReceiptVerification> VerifyReceiptOutcomeAsync(ReadOnlyMemory<byte> canonicalReceipt, AuthorizedReceiptBatch authorized, CancellationToken cancellationToken = default)
    {
        cancellationToken.ThrowIfCancellationRequested();
        var decoded = DecodeProtocolReceipt(canonicalReceipt.Span);
        var receipt = decoded.Receipt;
        if (receipt.ProtocolVersion != CurrentProtocolVersion) throw VerificationFailure(ReceiptCheck.ProtocolVersion);
        if (receipt.Operation == 0) throw VerificationFailure(ReceiptCheck.Operation);
        if (AllZero(receipt.ActivityId)) throw VerificationFailure(ReceiptCheck.ActivityId);
        if (AllZero(receipt.Asset)) throw VerificationFailure(ReceiptCheck.Asset);
        if (!Equal(receipt.BatchId, Exact(authorized.BatchId, 32))) throw VerificationFailure(ReceiptCheck.BatchId);
        if (!Equal(receipt.Asset, Exact(authorized.Asset, 32))) throw VerificationFailure(ReceiptCheck.Asset);
        if (!Equal(receipt.PreviousStateRoot, Exact(authorized.PreviousStateRoot, 32)))
            throw VerificationFailure(ReceiptCheck.PreviousStateRoot);
        if (!Equal(receipt.ResultingStateRoot, Exact(authorized.ResultingStateRoot, 32)))
            throw VerificationFailure(ReceiptCheck.ResultingStateRoot);
        if (receipt.ResultCode == 0)
        {
            if (!receipt.FromBalanceBefore.TrySubtract(receipt.Amount, out var debitAfter) || debitAfter != receipt.FromBalanceAfter)
                throw VerificationFailure(ReceiptCheck.DebitBalance);
            if (!receipt.ToBalanceBefore.TryAdd(receipt.Amount, out var creditAfter) || creditAfter != receipt.ToBalanceAfter)
                throw VerificationFailure(ReceiptCheck.CreditBalance);
        }
        var receiptDigest = Digest(ReceiptDomain, decoded.UnsignedBytes);
        if (!VerifyEd25519(Exact(authorized.SequencerPublicKey, 32), receipt.SequencerSignature, receiptDigest))
            throw VerificationFailure(ReceiptCheck.SequencerSignature);
        return ValueTask.FromResult(new ReceiptVerification("sequencer-signed", receipt, canonicalReceipt.ToArray(), receiptDigest));
    }

    public static async ValueTask<ReceiptVerification> VerifyReceiptAsync(ReadOnlyMemory<byte> canonicalReceipt, AuthorizedReceiptBatch authorized, CancellationToken cancellationToken = default)
    {
        var verified = await VerifyReceiptOutcomeAsync(canonicalReceipt, authorized, cancellationToken).ConfigureAwait(false);
        if (verified.Receipt.ResultCode != 0) throw VerificationFailure(ReceiptCheck.ResultCode);
        return verified;
    }

    private static DecodedReceipt DecodeProtocolReceipt(ReadOnlySpan<byte> canonicalReceipt)
    {
        try { return DecodeProtocolReceiptInner(canonicalReceipt); }
        catch (PlatformSdkException error) when (error.ReceiptCheck is not null) { throw; }
        catch (PlatformSdkException) { throw VerificationFailure(ReceiptCheck.Decode); }
    }

    private static DecodedReceipt DecodeProtocolReceiptInner(ReadOnlySpan<byte> canonicalReceipt)
    {
        if (canonicalReceipt.IsEmpty || canonicalReceipt.Length > MaximumMessageBytes)
            throw VerificationFailure(ReceiptCheck.ReceiptShape);
        var decoder = new WireDecoder(canonicalReceipt.ToArray());
        var envelopeVersion = decoder.U16();
        if (envelopeVersion is not (1 or 2) || decoder.U16() != 0x5201)
            throw VerificationFailure(ReceiptCheck.Decode);
        var protocolVersion = decoder.U16();
        if (protocolVersion != envelopeVersion) throw VerificationFailure(ReceiptCheck.ProtocolVersion);
        var activityId = decoder.Array32();
        var globalSequence = decoder.U64();
        var previousStateRoot = decoder.Array32();
        var resultingStateRoot = decoder.Array32();
        var activityRoot = decoder.Array32();
        var resultCode = decoder.I32();
        var effectCount = decoder.U32();
        if (effectCount > MaximumEffects) throw VerificationFailure();
        var effects = new List<ReceiptEffect>((int)effectCount);
        for (uint index = 0; index < effectCount; index++)
        {
            var moduleId = decoder.U16();
            var ordinal = decoder.U16();
            var eventType = decoder.U16();
            var kind = decoder.U8();
            var monetary = decoder.U8();
            if (kind is < 1 or > 3 || monetary > 1 || monetary == 1 && kind != 2) throw VerificationFailure();
            effects.Add(new(moduleId, ordinal, eventType, kind, monetary == 1, decoder.Array32(), decoder.Bounded(MaximumEffectBody)));
        }
        var feeCharged = decoder.U128();
        var batchId = decoder.Array32();
        var module = decoder.U16();
        var moduleVersion = decoder.U32();
        var parameterVersion = decoder.U32();
        var operation = decoder.U8();
        var asset = decoder.Array32();
        var amount = decoder.U128();
        var from = decoder.Array32();
        var fromBalanceBefore = decoder.U128();
        var fromBalanceAfter = decoder.U128();
        var fromSequence = decoder.U64();
        var to = decoder.Array32();
        var toBalanceBefore = decoder.U128();
        var toBalanceAfter = decoder.U128();
        var transferSetRoot = decoder.Array32();
        var authorizationHash = decoder.Array32();
        var contextHash = decoder.Array32();
        var timestamp = decoder.U64();
        if (globalSequence == 0) throw VerificationFailure(ReceiptCheck.GlobalSequence);
        if (module == 0) throw VerificationFailure(ReceiptCheck.ModuleId);
        if (moduleVersion == 0) throw VerificationFailure(ReceiptCheck.ModuleVersion);
        if (timestamp == 0) throw VerificationFailure(ReceiptCheck.Timestamp);
        if (AllZero(activityId)) throw VerificationFailure(ReceiptCheck.ActivityId);
        if (AllZero(resultingStateRoot)) throw VerificationFailure(ReceiptCheck.ResultingStateRoot);
        ProgramReceiptOutcome? programOutcome;
        try { programOutcome = decoder.Remaining > 69 ? DecodeProgramReceiptOutcomeFrom(decoder, protocolVersion) : null; }
        catch (PlatformSdkException) { throw VerificationFailure(ReceiptCheck.ProgramOutcome); }
        if (programOutcome is not null &&
            (module != GeneratedReceiptContract.ProgramsModuleId || programOutcome.ResultCode != resultCode ||
             programOutcome.TerminalKind == 1 && !Equal(programOutcome.TransferRoot, transferSetRoot) ||
             programOutcome.TerminalKind != 1 && !AllZero(transferSetRoot)))
            throw VerificationFailure(ReceiptCheck.ProgramOutcome);
        var signatureFlagOffset = decoder.Position;
        if (decoder.U8() != 1) throw VerificationFailure(ReceiptCheck.MissingSignature);
        var sequencerSignature = decoder.BoundedExactly(64);
        decoder.Finish();
        var receipt = new ProtocolReceipt(protocolVersion, activityId, globalSequence, previousStateRoot, resultingStateRoot, activityRoot, resultCode, effects.AsReadOnly(), feeCharged, batchId, module, moduleVersion, parameterVersion, operation, asset, amount, from, fromBalanceBefore, fromBalanceAfter, fromSequence, to, toBalanceBefore, toBalanceAfter, transferSetRoot, authorizationHash, contextHash, timestamp, programOutcome, sequencerSignature);
        var unsignedBytes = new byte[signatureFlagOffset + 1];
        canonicalReceipt[..signatureFlagOffset].CopyTo(unsignedBytes);
        return new(receipt, unsignedBytes);
    }

    private static ProgramReceiptOutcome DecodeProgramReceiptOutcomeFrom(WireDecoder decoder, ushort protocolVersion)
    {
        var encodingVersion = decoder.U32() switch
        {
            ProgramOutcomeV1 => (byte)1,
            ProgramOutcomeV2 => (byte)2,
            ProgramOutcomeV3 => (byte)3,
            _ => throw VerificationFailure(),
        };
        var terminalKind = decoder.U8();
        var resultCode = decoder.I32();
        var runtimeVersion = decoder.U16();
        var abiVersion = decoder.U16();
        var feeScheduleVersion = decoder.U32();
        var meteringScheduleVersion = encodingVersion == 3 ? decoder.U32() : 1U;
        var cpuFuel = decoder.U64();
        var memoryBytes = decoder.U64();
        var storageReadBytes = decoder.U64();
        var storageWriteBytes = decoder.U64();
        var outputValues = decoder.U32();
        var outputBytes = decoder.U64();
        var occupancyByteBatches = encodingVersion >= 2 ? decoder.U128() : default(UInt128Value);
        var occupancyFeeUnits = encodingVersion >= 2 ? decoder.U128() : default(UInt128Value);
        var feeSchedulePrices = new ulong[7];
        if (encodingVersion >= 2)
            for (var index = 0; index < feeSchedulePrices.Length; index++) feeSchedulePrices[index] = decoder.U64();
        var occupancyAssetId = encodingVersion >= 2 ? decoder.Array32() : new byte[32];
        var occupancyEvidenceDigest = encodingVersion >= 2 ? decoder.Array32() : new byte[32];
        var occupancyTransferRoot = encodingVersion >= 2 ? decoder.Array32() : new byte[32];
        var feeUnits = decoder.U128();
        var callGraphRoot = decoder.Array32();
        var terminalPayloadRoot = decoder.Array32();
        var transferRoot = decoder.Array32();
        var occupancyZero = occupancyByteBatches == default(UInt128Value) && occupancyFeeUnits == default(UInt128Value) &&
            AllZero(occupancyAssetId) && AllZero(occupancyEvidenceDigest) && AllZero(occupancyTransferRoot);
        var validVersion = protocolVersion == 1 && encodingVersion is 1 or 3 ||
            protocolVersion == 2 && encodingVersion is 2 or 3;
        if (terminalKind is < 1 or > 3 || runtimeVersion == 0 || abiVersion == 0 ||
            feeScheduleVersion == 0 || meteringScheduleVersion != 1 || AllZero(terminalPayloadRoot) ||
            terminalKind == 1 && resultCode != 0 ||
            terminalKind != 1 && (resultCode == 0 || resultCode <= -1000) ||
            terminalKind != 1 && !AllZero(transferRoot) || !validVersion ||
            encodingVersion == 1 && !occupancyZero ||
            encodingVersion >= 2 && terminalKind != 1 && !occupancyZero ||
            encodingVersion == 2 && terminalKind == 1 && (AllZero(occupancyAssetId) || AllZero(occupancyEvidenceDigest)) ||
            encodingVersion == 3 && AllZero(occupancyAssetId) != AllZero(occupancyEvidenceDigest) ||
            protocolVersion == 1 && encodingVersion == 3 && !occupancyZero ||
            protocolVersion == 2 && encodingVersion == 3 && terminalKind == 1 && (AllZero(occupancyAssetId) || AllZero(occupancyEvidenceDigest)))
            throw VerificationFailure();
        return new(encodingVersion, terminalKind, resultCode, runtimeVersion, abiVersion,
            feeScheduleVersion, meteringScheduleVersion, cpuFuel, memoryBytes, storageReadBytes,
            storageWriteBytes, outputValues, outputBytes, occupancyByteBatches, occupancyFeeUnits,
            Array.AsReadOnly(feeSchedulePrices), occupancyAssetId, occupancyEvidenceDigest,
            occupancyTransferRoot, feeUnits, callGraphRoot, terminalPayloadRoot, transferRoot);
    }

    public static ProgramReceiptOutcome DecodeProgramReceiptOutcome(ReadOnlySpan<byte> canonicalOutcome,
                                                                     ushort protocolVersion)
    {
        if (canonicalOutcome.IsEmpty || canonicalOutcome.Length > MaximumMessageBytes)
            throw VerificationFailure(ReceiptCheck.ReceiptShape);
        try
        {
            var decoder = new WireDecoder(canonicalOutcome.ToArray());
            var outcome = DecodeProgramReceiptOutcomeFrom(decoder, protocolVersion);
            decoder.Finish();
            return outcome;
        }
        catch (PlatformSdkException error) when (error.ReceiptCheck is not null) { throw; }
        catch (PlatformSdkException) { throw VerificationFailure(ReceiptCheck.ProgramOutcome); }
    }

    private static bool VerifyEd25519(ReadOnlySpan<byte> publicKey, ReadOnlySpan<byte> signature, ReadOnlySpan<byte> message)
    {
        if (publicKey.Length != 32 || signature.Length != 64 || message.Length != 32) return false;
        try
        {
            var verifier = new Ed25519Signer();
            verifier.Init(false, new Ed25519PublicKeyParameters(publicKey.ToArray(), 0));
            var encodedMessage = message.ToArray();
            verifier.BlockUpdate(encodedMessage, 0, encodedMessage.Length);
            return verifier.VerifySignature(signature.ToArray());
        }
        catch (ArgumentException) { return false; }
    }

    private static byte[] Digest(params byte[][] values)
    {
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        foreach (var value in values) hash.AppendData(value);
        return hash.GetHashAndReset();
    }

    private static byte[] Exact(byte[]? value, int length)
    {
        if (value is null || value.Length != length) throw VerificationFailure();
        return value.ToArray();
    }

    private static bool Equal(ReadOnlySpan<byte> left, ReadOnlySpan<byte> right) =>
        left.Length == right.Length && CryptographicOperations.FixedTimeEquals(left, right);

    private static bool AllZero(ReadOnlySpan<byte> value)
    {
        byte aggregate = 0;
        foreach (var item in value) aggregate |= item;
        return aggregate == 0;
    }

    private static int ProofDepth(uint count)
    {
        var depth = 0;
        while (count > 1) { count = count / 2 + count % 2; depth++; }
        return depth;
    }

    private static byte[] Concatenate(params byte[][] values)
    {
        var length = values.Aggregate(0, (sum, value) => checked(sum + value.Length));
        var result = new byte[length];
        var offset = 0;
        foreach (var value in values) { value.CopyTo(result, offset); offset += value.Length; }
        return result;
    }

    private static byte[] EncodeUInt32(uint value)
    {
        var encoded = new byte[4];
        BinaryPrimitives.WriteUInt32BigEndian(encoded, value);
        return encoded;
    }

    private static byte[] EncodeUInt16(ushort value)
    {
        var encoded = new byte[2];
        BinaryPrimitives.WriteUInt16BigEndian(encoded, value);
        return encoded;
    }

    private static byte[] EncodeUInt64(ulong value)
    {
        var encoded = new byte[8];
        BinaryPrimitives.WriteUInt64BigEndian(encoded, value);
        return encoded;
    }

    private static PlatformSdkException VerificationFailure() => new(SdkErrorCode.VerificationFailure, RetryClass.Never);
    private static PlatformSdkException VerificationFailure(ReceiptCheck check) =>
        new(SdkErrorCode.VerificationFailure, RetryClass.Never, receiptCheck: check);

    private sealed record DecodedReceipt(ProtocolReceipt Receipt, byte[] UnsignedBytes);

    private sealed class WireDecoder
    {
        private readonly byte[] _value;
        public int Position { get; private set; }
        public int Remaining => _value.Length - Position;

        public WireDecoder(byte[] value) => _value = value;

        public byte[] Fixed(int length)
        {
            if (length < 0 || Position > _value.Length - length) throw VerificationFailure();
            var result = _value.AsSpan(Position, length).ToArray();
            Position += length;
            return result;
        }

        public byte U8() => Fixed(1)[0];
        public ushort U16() => BinaryPrimitives.ReadUInt16BigEndian(Fixed(2));
        public uint U32() => BinaryPrimitives.ReadUInt32BigEndian(Fixed(4));
        public int I32() => BinaryPrimitives.ReadInt32BigEndian(Fixed(4));
        public ulong U64() => BinaryPrimitives.ReadUInt64BigEndian(Fixed(8));
        public UInt128Value U128() => new(U64(), U64());

        public byte[] Bounded(uint maximum)
        {
            var length = U32();
            if (length > maximum || length > int.MaxValue) throw VerificationFailure();
            return Fixed((int)length);
        }

        public byte[] BoundedExactly(uint length)
        {
            var value = Bounded(length);
            if (value.Length != length) throw VerificationFailure();
            return value;
        }

        public byte[] Array32() => BoundedExactly(32);
        public void Finish() { if (Position != _value.Length) throw VerificationFailure(); }
    }
}
