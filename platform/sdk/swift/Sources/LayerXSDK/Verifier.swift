import Crypto
import Foundation

private let merkleLeafDomain = Data("LXP/v1/merkle-leaf\0".utf8)
private let merkleInternalDomain = Data("LXP/v1/merkle-internal\0".utf8)
private let batchHeaderDomain = Data("LXP/v1/batch-header\0".utf8)
private let receiptDomain = Data("LXP/v1/receipt\0".utf8)
private let checkpointDomain = Data("LXP/v1/checkpoint-certificate\0".utf8)
private let guarantorAttestationDomain = Data("LXP/v1/guarantor-attestation\0".utf8)
private let maximumMessageBytes = 1_048_576
private let maximumEffects: UInt32 = 512
private let maximumEffectBody: UInt32 = 256
private let batchHeaderBytes = 354
private let allAvailabilityClasses: UInt8 = 0x1f

public struct UInt128Value: Hashable, Sendable {
    public let high: UInt64
    public let low: UInt64

    public init(high: UInt64, low: UInt64) {
        self.high = high
        self.low = low
    }

    fileprivate func subtracting(_ other: Self) -> Self? {
        let (low, lowUnderflow) = low.subtractingReportingOverflow(other.low)
        let (highAfterValue, highUnderflow) = high.subtractingReportingOverflow(other.high)
        let (resultHigh, borrowUnderflow) = highAfterValue.subtractingReportingOverflow(lowUnderflow ? 1 : 0)
        guard !highUnderflow, !borrowUnderflow else { return nil }
        return Self(high: resultHigh, low: low)
    }

    fileprivate func adding(_ other: Self) -> Self? {
        let (low, lowOverflow) = low.addingReportingOverflow(other.low)
        let (highAfterValue, highOverflow) = high.addingReportingOverflow(other.high)
        let (resultHigh, carryOverflow) = highAfterValue.addingReportingOverflow(lowOverflow ? 1 : 0)
        guard !highOverflow, !carryOverflow else { return nil }
        return Self(high: resultHigh, low: low)
    }
}

public struct MerkleProof: Sendable {
    public let leafIndex: UInt32
    public let leafCount: UInt32
    public let siblings: [Data]

    public init(leafIndex: UInt32, leafCount: UInt32, siblings: [Data]) {
        self.leafIndex = leafIndex
        self.leafCount = leafCount
        self.siblings = siblings
    }
}

public struct BatchHeader: Sendable {
    public let protocolVersion: UInt16
    public let networkID: UInt32
    public let epoch: UInt64
    public let batchNumber: UInt64
    public let firstSequence: UInt64
    public let lastSequence: UInt64
    public let previousStateRoot: Data
    public let resultingStateRoot: Data
    public let activityMerkleRoot: Data
    public let receiptMerkleRoot: Data
    public let eventMerkleRoot: Data
    public let dataAvailabilityRoot: Data
    public let oracleRoot: Data
    public let timestampMilliseconds: UInt64
    public let sequencerID: Data
}

public struct SequencerAuthorization: Sendable {
    public let sequencerID: Data
    public let publicKey: Data
    public let firstBatchNumber: UInt64
    public let lastBatchNumber: UInt64

    public init(sequencerID: Data, publicKey: Data, firstBatchNumber: UInt64, lastBatchNumber: UInt64) {
        self.sequencerID = sequencerID
        self.publicKey = publicKey
        self.firstBatchNumber = firstBatchNumber
        self.lastBatchNumber = lastBatchNumber
    }
}

public enum InclusionKind: Sendable, Equatable { case activity, receipt, event, state }

public struct InclusionVerification: Sendable {
    public let level: String
    public let header: BatchHeader
    public let headerDigest: Data
    public let root: Data
}

public struct CheckpointAttestation: Sendable {
    public let protocolVersion: UInt16
    public let networkID: UInt32
    public let paxeerChainID: UInt64
    public let settlementContract: Data
    public let epoch: UInt64
    public let checkpointID: Data
    public let checkpointHash: Data
    public let guarantorID: Data
    public let batchNumber: UInt64
    public let dataAvailabilityRoot: Data
    public let replayed: Bool
    public let dataPossessed: Bool
    public let availabilityClassMask: UInt8
    public let attestedAtMilliseconds: UInt64
    public let signature: Data

    public init(protocolVersion: UInt16, networkID: UInt32, paxeerChainID: UInt64, settlementContract: Data, epoch: UInt64, checkpointID: Data, checkpointHash: Data, guarantorID: Data, batchNumber: UInt64, dataAvailabilityRoot: Data, replayed: Bool, dataPossessed: Bool, availabilityClassMask: UInt8, attestedAtMilliseconds: UInt64, signature: Data) {
        self.protocolVersion = protocolVersion; self.networkID = networkID
        self.paxeerChainID = paxeerChainID; self.settlementContract = settlementContract; self.epoch = epoch
        self.checkpointID = checkpointID; self.checkpointHash = checkpointHash; self.guarantorID = guarantorID
        self.batchNumber = batchNumber; self.dataAvailabilityRoot = dataAvailabilityRoot
        self.replayed = replayed; self.dataPossessed = dataPossessed
        self.availabilityClassMask = availabilityClassMask; self.attestedAtMilliseconds = attestedAtMilliseconds
        self.signature = signature
    }
}

public struct GuarantorKey: Sendable {
    public let guarantorID: Data
    public let publicKey: Data
    public let bonded: Bool

    public init(guarantorID: Data, publicKey: Data, bonded: Bool) {
        self.guarantorID = guarantorID; self.publicKey = publicKey; self.bonded = bonded
    }
}

public struct CheckpointCertificate: Sendable {
    public let canonicalHeader: Data
    public let validityProof: Data
    public let attestations: [CheckpointAttestation]
    public let threshold: UInt32
    public let settlementReference: Data?

    public init(canonicalHeader: Data, validityProof: Data, attestations: [CheckpointAttestation], threshold: UInt32, settlementReference: Data? = nil) {
        self.canonicalHeader = canonicalHeader; self.validityProof = validityProof
        self.attestations = attestations; self.threshold = threshold; self.settlementReference = settlementReference
    }
}

public struct CheckpointVerificationInput: Sendable {
    public let certificate: CheckpointCertificate
    public let bondedSet: [GuarantorKey]
    public let registeredCheckpointID: Data
    public let registeredSettlementReference: Data?
    public let availabilityObtained: Bool

    public init(certificate: CheckpointCertificate, bondedSet: [GuarantorKey], registeredCheckpointID: Data, registeredSettlementReference: Data? = nil, availabilityObtained: Bool) {
        self.certificate = certificate; self.bondedSet = bondedSet
        self.registeredCheckpointID = registeredCheckpointID
        self.registeredSettlementReference = registeredSettlementReference
        self.availabilityObtained = availabilityObtained
    }
}

public protocol LocalSignatureVerifier: Sendable {
    func verifySecp256k1(publicKey: Data, signature: Data, digest: Data) async -> Bool
}

public struct CheckpointVerification: Sendable {
    public let level: String
    public let checkpointID: Data
    public let achieved: UInt32
    public let required: UInt32
    public let header: BatchHeader
}

public struct ReceiptEffect: Sendable {
    public let moduleID: UInt16
    public let ordinal: UInt16
    public let eventType: UInt16
    public let kind: UInt8
    public let monetary: Bool
    public let transferSetRoot: Data
    public let body: Data
}

public struct ProtocolReceipt: Sendable {
    public let protocolVersion: UInt16
    public let activityID: Data
    public let globalSequence: UInt64
    public let previousStateRoot: Data
    public let resultingStateRoot: Data
    public let activityRoot: Data
    public let resultCode: Int32
    public let effects: [ReceiptEffect]
    public let feeCharged: UInt128Value
    public let batchID: Data
    public let moduleID: UInt16
    public let moduleVersion: UInt32
    public let parameterVersion: UInt32
    public let operation: UInt8
    public let asset: Data
    public let amount: UInt128Value
    public let from: Data
    public let fromBalanceBefore: UInt128Value
    public let fromBalanceAfter: UInt128Value
    public let fromSequence: UInt64
    public let to: Data
    public let toBalanceBefore: UInt128Value
    public let toBalanceAfter: UInt128Value
    public let transferSetRoot: Data
    public let authorizationHash: Data
    public let contextHash: Data
    public let timestamp: UInt64
    public let sequencerSignature: Data
}

public struct AuthorizedReceiptBatch: Sendable {
    public let batchID: Data
    public let asset: Data
    public let previousStateRoot: Data
    public let resultingStateRoot: Data
    public let sequencerPublicKey: Data

    public init(batchID: Data, asset: Data, previousStateRoot: Data, resultingStateRoot: Data, sequencerPublicKey: Data) {
        self.batchID = batchID; self.asset = asset; self.previousStateRoot = previousStateRoot
        self.resultingStateRoot = resultingStateRoot; self.sequencerPublicKey = sequencerPublicKey
    }
}

public struct ReceiptVerification: Sendable {
    public let level: String
    public let receipt: ProtocolReceipt
    public let canonicalBytes: Data
    public let receiptDigest: Data
}

public enum LocalVerifier {
    public static func verifyMerkleInclusion(canonicalLeaf: Data, proof: MerkleProof, expectedRoot: Data) throws {
        guard proof.leafCount > 0, proof.leafIndex < proof.leafCount,
              proof.siblings.count <= 32, proof.siblings.count == proofDepth(proof.leafCount),
              expectedRoot.count == 32 else { throw verificationFailure() }
        var current = digest(merkleLeafDomain, canonicalLeaf)
        var index = proof.leafIndex
        var count = proof.leafCount
        for untrustedSibling in proof.siblings {
            let sibling = try exact(untrustedSibling, 32)
            if (index ^ 1) >= count, sibling != current { throw verificationFailure() }
            current = index % 2 == 0
                ? digest(merkleInternalDomain, current, sibling)
                : digest(merkleInternalDomain, sibling, current)
            index /= 2
            count = count / 2 + count % 2
        }
        guard current == expectedRoot else { throw verificationFailure() }
    }

    public static func decodeBatchHeader(_ canonicalHeader: Data) throws -> BatchHeader {
        guard canonicalHeader.count == batchHeaderBytes else { throw verificationFailure() }
        var decoder = WireDecoder(canonicalHeader)
        guard try decoder.u16() == 1, try decoder.u16() == 0x1701, try decoder.u8() == 15 else { throw verificationFailure() }
        func field(_ expected: UInt8) throws { guard try decoder.u8() == expected else { throw verificationFailure() } }
        try field(1); let protocolVersion = try decoder.u16()
        try field(2); let networkID = try decoder.u32()
        try field(3); let epoch = try decoder.u64()
        try field(4); let batchNumber = try decoder.u64()
        try field(5); let firstSequence = try decoder.u64()
        try field(6); let lastSequence = try decoder.u64()
        try field(7); let previousStateRoot = try decoder.array32()
        try field(8); let resultingStateRoot = try decoder.array32()
        try field(9); let activityMerkleRoot = try decoder.array32()
        try field(10); let receiptMerkleRoot = try decoder.array32()
        try field(11); let eventMerkleRoot = try decoder.array32()
        try field(12); let dataAvailabilityRoot = try decoder.array32()
        try field(13); let oracleRoot = try decoder.array32()
        try field(14); let timestampMilliseconds = try decoder.u64()
        try field(15); let sequencerID = try decoder.array32()
        try decoder.finish()
        return BatchHeader(protocolVersion: protocolVersion, networkID: networkID, epoch: epoch, batchNumber: batchNumber, firstSequence: firstSequence, lastSequence: lastSequence, previousStateRoot: previousStateRoot, resultingStateRoot: resultingStateRoot, activityMerkleRoot: activityMerkleRoot, receiptMerkleRoot: receiptMerkleRoot, eventMerkleRoot: eventMerkleRoot, dataAvailabilityRoot: dataAvailabilityRoot, oracleRoot: oracleRoot, timestampMilliseconds: timestampMilliseconds, sequencerID: sequencerID)
    }

    public static func verifyBatchInclusion(kind: InclusionKind, canonicalLeaf: Data, proof: MerkleProof, canonicalHeader: Data, headerSignature: Data, authorization: SequencerAuthorization) async throws -> InclusionVerification {
        let header = try decodeBatchHeader(canonicalHeader)
        let authorizedSequencerID = try exact(authorization.sequencerID, 32)
        guard header.batchNumber >= authorization.firstBatchNumber,
              header.batchNumber <= authorization.lastBatchNumber,
              header.sequencerID == authorizedSequencerID else { throw verificationFailure() }
        let headerDigest = digest(batchHeaderDomain, canonicalHeader)
        guard verifyEd25519(publicKey: authorization.publicKey, signature: headerSignature, message: headerDigest) else { throw verificationFailure() }
        let root: Data
        switch kind {
        case .activity: root = header.activityMerkleRoot
        case .receipt: root = header.receiptMerkleRoot
        case .event: root = header.eventMerkleRoot
        case .state: root = header.resultingStateRoot
        }
        try verifyMerkleInclusion(canonicalLeaf: canonicalLeaf, proof: proof, expectedRoot: root)
        return InclusionVerification(level: kind == .state ? "state-proven" : "batch-included", header: header, headerDigest: headerDigest, root: root)
    }

    public static func verifyCheckpoint(_ input: CheckpointVerificationInput, signatures: LocalSignatureVerifier) async throws -> CheckpointVerification {
        let certificate = input.certificate
        guard input.availabilityObtained, certificate.threshold > 0,
              UInt64(certificate.validityProof.count) <= UInt64(UInt32.max) else { throw verificationFailure() }
        let header = try decodeBatchHeader(certificate.canonicalHeader)
        let checkpointID = digest(checkpointDomain, certificate.canonicalHeader, encodeUInt32(UInt32(certificate.validityProof.count)), certificate.validityProof)
        let registeredCheckpointID = try exact(input.registeredCheckpointID, 32)
        guard checkpointID == registeredCheckpointID else { throw verificationFailure() }
        var bonded: [Data: GuarantorKey] = [:]
        for member in input.bondedSet where member.bonded {
            bonded[try exact(member.guarantorID, 32)] = member
        }
        var seen: Set<Data> = []
        var achieved: UInt32 = 0
        var paxeerChainID: UInt64?
        var settlementContract: Data?
        for attestation in certificate.attestations {
            let guarantorID = try exact(attestation.guarantorID, 32)
            let attestationSettlementContract = try exact(attestation.settlementContract, 20)
            guard seen.insert(guarantorID).inserted,
                  attestation.protocolVersion == header.protocolVersion,
                  attestation.networkID == header.networkID,
                  attestation.epoch == header.epoch,
                  attestation.paxeerChainID > 0,
                  !allZero(attestationSettlementContract),
                  paxeerChainID == nil || (attestation.paxeerChainID == paxeerChainID && attestationSettlementContract == settlementContract),
                  try exact(attestation.checkpointID, 32) == checkpointID,
                  try exact(attestation.checkpointHash, 32) == checkpointID,
                  attestation.batchNumber == header.batchNumber,
                  try exact(attestation.dataAvailabilityRoot, 32) == header.dataAvailabilityRoot,
                  attestation.replayed, attestation.dataPossessed,
                  attestation.availabilityClassMask == allAvailabilityClasses,
                  attestation.attestedAtMilliseconds > 0,
                  let member = bonded[guarantorID], achieved < UInt32.max else { throw verificationFailure() }
            paxeerChainID = attestation.paxeerChainID
            settlementContract = attestationSettlementContract
            let message = try concatenate(
                encodeUInt16(attestation.protocolVersion), encodeUInt32(attestation.networkID), encodeUInt64(attestation.paxeerChainID),
                attestationSettlementContract, encodeUInt64(attestation.epoch),
                exact(attestation.checkpointID, 32), exact(attestation.checkpointHash, 32), guarantorID,
                encodeUInt64(attestation.batchNumber), exact(attestation.dataAvailabilityRoot, 32),
                Data([1, 1, attestation.availabilityClassMask]), encodeUInt64(attestation.attestedAtMilliseconds)
            )
            let attestationDigest = digest(guarantorAttestationDomain, message)
            guard await signatures.verifySecp256k1(
                publicKey: try exact(member.publicKey, 33),
                signature: try exact(attestation.signature, 64),
                digest: attestationDigest
            ) else { throw verificationFailure() }
            achieved += 1
        }
        guard achieved >= certificate.threshold else { throw verificationFailure() }
        let level: String
        if let settlement = certificate.settlementReference {
            guard !settlement.isEmpty, let registered = input.registeredSettlementReference, settlement == registered else { throw verificationFailure() }
            level = "settlement-anchored"
        } else {
            level = "checkpoint-finalised"
        }
        return CheckpointVerification(level: level, checkpointID: checkpointID, achieved: achieved, required: certificate.threshold, header: header)
    }

    public static func verifyReceiptOutcome(_ canonicalReceipt: Data, authorized: AuthorizedReceiptBatch) async throws -> ReceiptVerification {
        let decoded = try decodeProtocolReceipt(canonicalReceipt)
        let receipt = decoded.receipt
        let batchID = try exact(authorized.batchID, 32)
        let asset = try exact(authorized.asset, 32)
        let previousStateRoot = try exact(authorized.previousStateRoot, 32)
        let resultingStateRoot = try exact(authorized.resultingStateRoot, 32)
        guard receipt.operation != 0, !allZero(receipt.activityID), !allZero(receipt.asset),
              receipt.batchID == batchID,
              receipt.asset == asset,
              receipt.previousStateRoot == previousStateRoot,
              receipt.resultingStateRoot == resultingStateRoot else { throw verificationFailure() }
        if receipt.resultCode == 0 {
            guard receipt.fromBalanceBefore.subtracting(receipt.amount) == receipt.fromBalanceAfter,
                  receipt.toBalanceBefore.adding(receipt.amount) == receipt.toBalanceAfter else { throw verificationFailure() }
        }
        let receiptDigest = digest(receiptDomain, decoded.unsignedBytes)
        guard verifyEd25519(publicKey: try exact(authorized.sequencerPublicKey, 32), signature: receipt.sequencerSignature, message: receiptDigest) else { throw verificationFailure() }
        return ReceiptVerification(level: "sequencer-signed", receipt: receipt, canonicalBytes: canonicalReceipt, receiptDigest: receiptDigest)
    }

    public static func verifyReceipt(_ canonicalReceipt: Data, authorized: AuthorizedReceiptBatch) async throws -> ReceiptVerification {
        let verified = try await verifyReceiptOutcome(canonicalReceipt, authorized: authorized)
        guard verified.receipt.resultCode == 0 else { throw verificationFailure() }
        return verified
    }
}

private struct DecodedReceipt {
    let receipt: ProtocolReceipt
    let unsignedBytes: Data
}

private func decodeProtocolReceipt(_ canonicalReceipt: Data) throws -> DecodedReceipt {
    guard !canonicalReceipt.isEmpty, canonicalReceipt.count <= maximumMessageBytes else { throw verificationFailure() }
    var decoder = WireDecoder(canonicalReceipt)
    guard try decoder.u16() == 1, try decoder.u16() == 0x5201 else { throw verificationFailure() }
    let protocolVersion = try decoder.u16()
    guard protocolVersion == 1 else { throw verificationFailure() }
    let activityID = try decoder.array32()
    let globalSequence = try decoder.u64()
    let previousStateRoot = try decoder.array32()
    let resultingStateRoot = try decoder.array32()
    let activityRoot = try decoder.array32()
    let resultCode = try decoder.i32()
    let effectCount = try decoder.u32()
    guard effectCount <= maximumEffects else { throw verificationFailure() }
    var effects: [ReceiptEffect] = []
    effects.reserveCapacity(Int(effectCount))
    for _ in 0..<effectCount {
        let moduleID = try decoder.u16()
        let ordinal = try decoder.u16()
        let eventType = try decoder.u16()
        let kind = try decoder.u8()
        let monetaryValue = try decoder.u8()
        guard (1...3).contains(kind), monetaryValue <= 1, monetaryValue == 0 || kind == 2 else { throw verificationFailure() }
        effects.append(ReceiptEffect(moduleID: moduleID, ordinal: ordinal, eventType: eventType, kind: kind, monetary: monetaryValue == 1, transferSetRoot: try decoder.array32(), body: try decoder.bounded(maximum: maximumEffectBody)))
    }
    let feeCharged = try decoder.u128()
    let batchID = try decoder.array32()
    let moduleID = try decoder.u16()
    let moduleVersion = try decoder.u32()
    let parameterVersion = try decoder.u32()
    let operation = try decoder.u8()
    let asset = try decoder.array32()
    let amount = try decoder.u128()
    let from = try decoder.array32()
    let fromBalanceBefore = try decoder.u128()
    let fromBalanceAfter = try decoder.u128()
    let fromSequence = try decoder.u64()
    let to = try decoder.array32()
    let toBalanceBefore = try decoder.u128()
    let toBalanceAfter = try decoder.u128()
    let transferSetRoot = try decoder.array32()
    let authorizationHash = try decoder.array32()
    let contextHash = try decoder.array32()
    let timestamp = try decoder.u64()
    let signatureFlagOffset = decoder.position
    guard try decoder.u8() == 1 else { throw verificationFailure() }
    let sequencerSignature = try decoder.bounded(exactly: 64)
    try decoder.finish()
    let receipt = ProtocolReceipt(protocolVersion: protocolVersion, activityID: activityID, globalSequence: globalSequence, previousStateRoot: previousStateRoot, resultingStateRoot: resultingStateRoot, activityRoot: activityRoot, resultCode: resultCode, effects: effects, feeCharged: feeCharged, batchID: batchID, moduleID: moduleID, moduleVersion: moduleVersion, parameterVersion: parameterVersion, operation: operation, asset: asset, amount: amount, from: from, fromBalanceBefore: fromBalanceBefore, fromBalanceAfter: fromBalanceAfter, fromSequence: fromSequence, to: to, toBalanceBefore: toBalanceBefore, toBalanceAfter: toBalanceAfter, transferSetRoot: transferSetRoot, authorizationHash: authorizationHash, contextHash: contextHash, timestamp: timestamp, sequencerSignature: sequencerSignature)
    var unsigned = canonicalReceipt.prefix(signatureFlagOffset)
    unsigned.append(0)
    return DecodedReceipt(receipt: receipt, unsignedBytes: Data(unsigned))
}

private struct WireDecoder {
    private let bytes: [UInt8]
    private(set) var position = 0

    init(_ data: Data) { bytes = Array(data) }

    mutating func fixed(_ count: Int) throws -> Data {
        guard count >= 0, position <= bytes.count - count else { throw verificationFailure() }
        let value = Data(bytes[position..<(position + count)])
        position += count
        return value
    }

    mutating func u8() throws -> UInt8 { try fixed(1)[0] }
    mutating func u16() throws -> UInt16 { try fixed(2).reduce(0) { ($0 << 8) | UInt16($1) } }
    mutating func u32() throws -> UInt32 { try fixed(4).reduce(0) { ($0 << 8) | UInt32($1) } }
    mutating func i32() throws -> Int32 { Int32(bitPattern: try u32()) }
    mutating func u64() throws -> UInt64 { try fixed(8).reduce(0) { ($0 << 8) | UInt64($1) } }
    mutating func u128() throws -> UInt128Value { UInt128Value(high: try u64(), low: try u64()) }

    mutating func bounded(maximum: UInt32) throws -> Data {
        let length = try u32()
        guard length <= maximum, UInt64(length) <= UInt64(Int.max) else { throw verificationFailure() }
        return try fixed(Int(length))
    }

    mutating func bounded(exactly length: UInt32) throws -> Data {
        let value = try bounded(maximum: length)
        guard value.count == Int(length) else { throw verificationFailure() }
        return value
    }

    mutating func array32() throws -> Data { try bounded(exactly: 32) }
    func finish() throws { guard position == bytes.count else { throw verificationFailure() } }
}

private func verificationFailure() -> PlatformSDKError {
    PlatformSDKError(code: .verificationFailure, retry: .never)
}

private func exact(_ value: Data, _ length: Int) throws -> Data {
    guard value.count == length else { throw verificationFailure() }
    return value
}

private func allZero(_ value: Data) -> Bool {
    value.reduce(UInt8(0)) { $0 | $1 } == 0
}

private func concatenate(_ values: Data...) throws -> Data {
    var result = Data()
    for value in values { result.append(value) }
    return result
}

private func digest(_ values: Data...) -> Data {
    var hasher = SHA256()
    for value in values { hasher.update(data: value) }
    return Data(hasher.finalize())
}

private func verifyEd25519(publicKey: Data, signature: Data, message: Data) -> Bool {
    guard publicKey.count == 32, signature.count == 64, message.count == 32,
          let key = try? Curve25519.Signing.PublicKey(rawRepresentation: publicKey) else { return false }
    return key.isValidSignature(signature, for: message)
}

private func proofDepth(_ count: UInt32) -> Int {
    var count = count
    var depth = 0
    while count > 1 { count = count / 2 + count % 2; depth += 1 }
    return depth
}

private func encodeUInt32(_ value: UInt32) -> Data {
    Data([
        UInt8(truncatingIfNeeded: value >> 24), UInt8(truncatingIfNeeded: value >> 16),
        UInt8(truncatingIfNeeded: value >> 8), UInt8(truncatingIfNeeded: value),
    ])
}

private func encodeUInt16(_ value: UInt16) -> Data {
    Data([UInt8(truncatingIfNeeded: value >> 8), UInt8(truncatingIfNeeded: value)])
}

private func encodeUInt64(_ value: UInt64) -> Data {
    Data((0..<8).map { UInt8(truncatingIfNeeded: value >> UInt64(56 - 8 * $0)) })
}
