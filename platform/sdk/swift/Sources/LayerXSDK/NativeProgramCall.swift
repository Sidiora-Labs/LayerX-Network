import Foundation

public struct NativeProgramCall: Sendable {
    public let programID: Data
    public let guestABI: UInt16
    public let entrypoint: String
    public let calldata: Data
    public let capabilities: Data
    public let accessDeclaration: Data
    public let responseCapacity: UInt32
    public let resources: [UInt64]

    public init(programID: Data, guestABI: UInt16, entrypoint: String, calldata: Data,
                capabilities: Data, accessDeclaration: Data, responseCapacity: UInt32, resources: [UInt64]) {
        self.programID = programID; self.guestABI = guestABI; self.entrypoint = entrypoint
        self.calldata = calldata; self.capabilities = capabilities; self.accessDeclaration = accessDeclaration
        self.responseCapacity = responseCapacity; self.resources = resources
    }

    public func encode() throws -> Data {
        let entry = Data(entrypoint.utf8)
        guard programID.count == 32, programID.contains(where: { $0 != 0 }), guestABI == 1 || guestABI == 2,
              !entry.isEmpty, entry.count <= 128,
              entry.allSatisfy({ (65...90).contains($0) || (97...122).contains($0) || (48...57).contains($0) || $0 == 95 || $0 == 46 }),
              calldata.count <= 1048576, capabilities.count <= 65535, accessDeclaration.count <= 1048576,
              responseCapacity <= 1048576, resources.count == 7 else { throw NativeProgramCallError.invalid }
        var output = programID
        func put(_ value: UInt64, _ width: Int) { for i in (0..<width).reversed() { output.append(UInt8(truncatingIfNeeded: value >> (i * 8))) } }
        put(UInt64(guestABI), 2); put(UInt64(entry.count), 2); put(UInt64(calldata.count), 4)
        put(UInt64(capabilities.count), 2); put(UInt64(accessDeclaration.count), 4); put(UInt64(responseCapacity), 4)
        for resource in resources { put(resource, 8) }
        for body in [entry, calldata, capabilities, accessDeclaration] { output.append(body) }
        return output
    }

    public static func decode(_ payload: Data) throws -> NativeProgramCall {
        let bytes = [UInt8](payload)
        guard bytes.count >= 106 else { throw NativeProgramCallError.invalid }
        func get(_ offset: Int, _ width: Int) -> UInt64 { bytes[offset..<offset + width].reduce(0) { ($0 << 8) | UInt64($1) } }
        let sizes = [get(34, 2), get(36, 4), get(40, 2), get(42, 4)]
        guard 106 + sizes.reduce(0, +) == UInt64(bytes.count) else { throw NativeProgramCallError.invalid }
        var offset = 106; var bodies: [Data] = []
        for size in sizes { bodies.append(Data(bytes[offset..<offset + Int(size)])); offset += Int(size) }
        guard let entry = String(data: bodies[0], encoding: .ascii) else { throw NativeProgramCallError.invalid }
        let call = NativeProgramCall(programID: Data(bytes[..<32]), guestABI: UInt16(get(32, 2)), entrypoint: entry,
            calldata: bodies[1], capabilities: bodies[2], accessDeclaration: bodies[3], responseCapacity: UInt32(get(46, 4)),
            resources: (0..<7).map { get(50 + $0 * 8, 8) })
        guard try call.encode() == payload else { throw NativeProgramCallError.invalid }
        return call
    }
}

public enum NativeProgramCallError: Error { case invalid }
