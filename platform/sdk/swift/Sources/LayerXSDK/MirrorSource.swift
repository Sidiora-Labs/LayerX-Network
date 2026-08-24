import Foundation

public struct MirrorCandidate: Sendable {
    public let source: Int
    public let commitment: Data
    public init(source: Int, commitment: Data) { self.source = source; self.commitment = commitment }
}
public struct MirrorPolicy: Sendable {
    public let kind: MirrorPolicyKind
    public let candidates: [MirrorCandidate]
    public let minimum: Int
    public init(kind: MirrorPolicyKind, candidates: [MirrorCandidate], minimum: Int = 1) { self.kind=kind;self.candidates=candidates;self.minimum=minimum }
}
public struct MirrorVerification: Sendable {
    public let level: String; public let batchNumber: UInt64; public let headerDigest: Data
    public let evidenceDigest: Data; public let sourceID: String; public let target: String
    public let canonicalPosition: String; public let provenance: String; public let latestBatch: UInt64?
    public let batchLag: String; public let failoverCount: Int; public let agreeingSources: Int
    public let checkpointLevel: String
}
public protocol MirrorSourceVerifying: Sendable {
    func receipt(batchNumber: UInt64, policy: MirrorPolicy, canonicalReceipt: Data) throws -> MirrorVerification
    func state(batchNumber: UInt64, policy: MirrorPolicy, canonicalState: Data, canonicalProof: Data) throws -> MirrorVerification
}

#if os(macOS) || os(Linux)
public final class LocalMirrorExecutableVerifier: MirrorSourceVerifying, @unchecked Sendable {
    private let executable: URL; private let configuration: URL; private let timeout: TimeInterval
    public init(executable: URL, configuration: URL, timeout: TimeInterval = 120) throws {
        guard executable.isFileURL, configuration.isFileURL, timeout >= 0.1, timeout <= 120 else { throw MirrorErrorCode.configuration }
        self.executable=executable;self.configuration=configuration;self.timeout=timeout
    }
    public func receipt(batchNumber: UInt64, policy: MirrorPolicy, canonicalReceipt: Data) throws -> MirrorVerification {
        try verify(batchNumber:batchNumber,policy:policy,evidence:["kind":"receipt","canonical_hex":canonicalReceipt.hex])
    }
    public func state(batchNumber: UInt64, policy: MirrorPolicy, canonicalState: Data, canonicalProof: Data) throws -> MirrorVerification {
        try verify(batchNumber:batchNumber,policy:policy,evidence:["kind":"state","canonical_hex":canonicalState.hex,"proof_hex":canonicalProof.hex])
    }
    private func verify(batchNumber: UInt64, policy: MirrorPolicy, evidence: [String:Any]) throws -> MirrorVerification {
        guard batchNumber>0,!policy.candidates.isEmpty,policy.candidates.count<=MirrorSchemaV2.maximumSources else{throw MirrorErrorCode.configuration}
        var seen=Set<Int>();let candidates=try policy.candidates.map{value->[String:Any] in guard value.source>=0,seen.insert(value.source).inserted,value.commitment.count==32 else{throw MirrorErrorCode.configuration};return["source":value.source,"commitment_hex":value.commitment.hex]}
        let wirePolicy:[String:Any];switch policy.kind{case .exact:guard candidates.count==1 else{throw MirrorErrorCode.configuration};wirePolicy=["kind":"exact","candidate":candidates[0]];case .orderedPreference:wirePolicy=["kind":"ordered-preference","candidates":candidates];case .agreement:guard policy.minimum>0,policy.minimum<=candidates.count else{throw MirrorErrorCode.configuration};wirePolicy=["kind":"agreement","candidates":candidates,"minimum":policy.minimum]}
        let request=try JSONSerialization.data(withJSONObject:["batch_number":batchNumber.description,"evidence":evidence,"policy":wirePolicy]);guard request.count<=40*1024*1024 else{throw MirrorErrorCode.bounds}
        let process=Process();let input=Pipe(),output=Pipe();process.executableURL=executable;process.arguments=[configuration.path];process.standardInput=input;process.standardOutput=output;process.standardError=FileHandle.nullDevice
        do{try process.run()}catch{throw MirrorErrorCode.unavailable};input.fileHandleForWriting.write(request);try? input.fileHandleForWriting.close()
        let deadline=Date().addingTimeInterval(timeout);while process.isRunning&&Date()<deadline{Thread.sleep(forTimeInterval:0.01)};if process.isRunning{process.terminate();throw MirrorErrorCode.unavailable}
        let bytes=output.fileHandleForReading.readDataToEndOfFile();guard bytes.count<=1_048_576,let response=try JSONSerialization.jsonObject(with:bytes)as?[String:Any] else{throw MirrorErrorCode.malformed};guard response["ok"]as?Bool==true else{throw MirrorErrorCode(rawValue:response["error"]as?String ?? "unavailable") ?? .unavailable};guard let value=response["verification"]as?[String:Any],let level=value["level"]as?String,let batch=uint64(value["batchNumber"]),let header=Data(hex:value["headerDigest"]as?String),let digest=Data(hex:value["evidenceDigest"]as?String),header.count==32,digest.count==32,let source=value["sourceId"]as?String,!source.isEmpty,source.utf8.count<=64,let target=value["target"]as?String,!target.isEmpty,target.utf8.count<=2048,let position=value["canonicalPosition"]as?String,!position.isEmpty,position.utf8.count<=2048,let provenance=value["provenance"]as?String,(provenance=="Canonical"||provenance=="Reorged"),let lag=value["batchLag"]as?String,let failover=(value["failoverCount"]as?NSNumber)?.intValue,let agreeing=(value["agreeingSources"]as?NSNumber)?.intValue,failover>=0,failover<=8,agreeing>0,agreeing<=8,let checkpoint=value["checkpointLevel"]as?String,checkpoint=="unavailable" else{throw MirrorErrorCode.malformed}
        let latest:UInt64?;if let raw=value["latestBatch"],!(raw is NSNull){guard let parsed=uint64(raw)else{throw MirrorErrorCode.malformed};latest=parsed}else{latest=nil}
        return MirrorVerification(level:level,batchNumber:batch,headerDigest:header,evidenceDigest:digest,sourceID:source,target:target,canonicalPosition:position,provenance:provenance,latestBatch:latest,batchLag:lag,failoverCount:failover,agreeingSources:agreeing,checkpointLevel:checkpoint)
    }
}
private extension Data{var hex:String{map{String(format:"%02x",$0)}.joined()};init?(hex:String?){guard let hex,hex.count%2==0 else{return nil};var value=Data();var index=hex.startIndex;while index<hex.endIndex{let end=hex.index(index,offsetBy:2);guard let byte=UInt8(hex[index..<end],radix:16)else{return nil};value.append(byte);index=end};self=value}}
private func uint64(_ value:Any?)->UInt64?{guard let text=value as? String,!text.isEmpty,text.first != "0",text.allSatisfy({$0.isASCII&&$0.isNumber}),let result=UInt64(text),result>0 else{return nil};return result}
#endif
