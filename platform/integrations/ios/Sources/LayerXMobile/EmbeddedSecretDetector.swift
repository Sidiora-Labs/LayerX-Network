import Foundation

public struct EmbeddedSecretFinding: Sendable, Equatable, CustomStringConvertible {
    public let rule: String
    public let path: String
    public let offset: Int
    public let length: Int

    public init(rule: String, path: String, offset: Int, length: Int) {
        self.rule = rule
        self.path = path
        self.offset = offset
        self.length = length
    }

    public var description: String { "\(path):\(offset) rule=\(rule) length=\(length)" }
}

public enum EmbeddedSecretDetector {
    private static let providerPrefixes: [(String, String)] = [
        ("openai-key", "sk-"),
        ("stripe-secret-key", "sk_live_"),
        ("stripe-restricted-key", "rk_live_"),
        ("aws-access-key", "AKIA"),
        ("aws-temporary-key", "ASIA"),
        ("github-token", "ghp_"),
        ("github-oauth-token", "gho_"),
        ("github-fine-grained-token", "github_pat_"),
        ("slack-bot-token", "xoxb-"),
        ("slack-user-token", "xoxp-"),
        ("google-api-key", "AIza"),
        ("sendgrid-key", "SG."),
        ("npm-token", "npm_"),
        ("gitlab-token", "glpat-"),
        ("huggingface-token", "hf_"),
        ("digitalocean-token", "dop_v1_"),
        ("shopify-token", "shpat_"),
        ("layerx-service-secret", "lxs_"),
    ]

    private static let pemMarkers: [(String, String)] = [
        ("pem-private-key", "-----BEGIN PRIVATE KEY-----"),
        ("pem-rsa-private-key", "-----BEGIN RSA PRIVATE KEY-----"),
        ("pem-ec-private-key", "-----BEGIN EC PRIVATE KEY-----"),
        ("pem-encrypted-private-key", "-----BEGIN ENCRYPTED PRIVATE KEY-----"),
        ("openssh-private-key", "-----BEGIN OPENSSH PRIVATE KEY-----"),
        ("pgp-private-key", "-----BEGIN PGP PRIVATE KEY BLOCK-----"),
    ]

    private static let secretKeyNames: [String] = [
        "secret", "api_key", "apikey", "api-key", "private_key", "privatekey", "private-key",
        "password", "passphrase", "client_secret", "access_token", "refresh_token", "bearer",
        "signing_key", "seed", "mnemonic", "credential", "authorization",
    ]

    public static func isSecretShapedName(_ name: String) -> Bool {
        let normalized = name.lowercased()
        return secretKeyNames.contains { normalized == $0 || normalized.hasSuffix(".\($0)") || normalized.contains($0) }
    }

    public static func providerCredentialRule(_ value: String) -> String? {
        for (rule, marker) in pemMarkers where value.contains(marker) {
            return rule
        }
        for (rule, prefix) in providerPrefixes where value.hasPrefix(prefix) && value.count >= prefix.count + 12 {
            return rule
        }
        return nil
    }

    public static func classify(_ value: String) -> String? {
        if let rule = providerCredentialRule(value) {
            return rule
        }
        if isSignedJSONWebToken(value) {
            return "signed-json-web-token"
        }
        if isHighEntropyMaterial(value) {
            return "high-entropy-material"
        }
        return nil
    }

    public static func isSignedJSONWebToken(_ value: String) -> Bool {
        let segments = value.split(separator: ".", omittingEmptySubsequences: false)
        guard segments.count == 3, segments[0].count >= 8, segments[1].count >= 8, segments[2].count >= 16 else {
            return false
        }
        for segment in segments where !segment.allSatisfy({ isBase64URLCharacter($0) }) {
            return false
        }
        guard let header = decodeBase64URL(String(segments[0])),
              let text = String(data: header, encoding: .utf8) else { return false }
        return text.contains("\"alg\"")
    }

    public static func isHighEntropyMaterial(_ value: String) -> Bool {
        guard value.count >= 40, value.count <= 4096 else { return false }
        let characters = Array(value.utf8)
        let base64Like = characters.allSatisfy { isBase64Character(Character(UnicodeScalar($0))) }
        let hexLike = characters.allSatisfy { isHexCharacter(Character(UnicodeScalar($0))) }
        guard base64Like || hexLike else { return false }
        if hexLike && value.count <= 64 { return false }
        return shannonEntropyBitsPerCharacter(characters) >= 3.6
    }

    public static func shannonEntropyBitsPerCharacter(_ bytes: [UInt8]) -> Double {
        guard !bytes.isEmpty else { return 0 }
        var counts = [Int](repeating: 0, count: 256)
        for byte in bytes { counts[Int(byte)] += 1 }
        let total = Double(bytes.count)
        var entropy = 0.0
        for count in counts where count > 0 {
            let probability = Double(count) / total
            entropy -= probability * (log(probability) / log(2.0))
        }
        return entropy
    }

    public static func scan(contentsOf data: Data, path: String, textual: Bool, exempt: Set<String>) -> [EmbeddedSecretFinding] {
        var findings: [EmbeddedSecretFinding] = []
        for run in printableRuns(in: data) {
            guard !exempt.contains(run.text) else { continue }
            if let rule = classifyRun(run.text, textual: textual) {
                findings.append(EmbeddedSecretFinding(rule: rule, path: path, offset: run.offset, length: run.text.utf8.count))
            }
        }
        return findings
    }

    public static func scan(fileAt url: URL, root: URL, exempt: Set<String>) throws -> [EmbeddedSecretFinding] {
        let data = try Data(contentsOf: url, options: [.mappedIfSafe])
        let relative = relativePath(of: url, from: root)
        return scan(contentsOf: data, path: relative, textual: isTextualArtifact(url), exempt: exempt)
    }

    public static func scan(directoryAt root: URL, exempt: Set<String>) throws -> [EmbeddedSecretFinding] {
        var findings: [EmbeddedSecretFinding] = []
        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: root.path, isDirectory: &isDirectory) else {
            throw MobileIntegrationError(.invalidConfiguration)
        }
        if !isDirectory.boolValue {
            return try scan(fileAt: root, root: root.deletingLastPathComponent(), exempt: exempt)
        }
        guard let walker = FileManager.default.enumerator(
            at: root,
            includingPropertiesForKeys: [.isRegularFileKey, .fileSizeKey],
            options: []
        ) else {
            throw MobileIntegrationError(.invalidConfiguration)
        }
        for case let candidate as URL in walker {
            let values = try candidate.resourceValues(forKeys: [.isRegularFileKey, .fileSizeKey])
            guard values.isRegularFile == true else { continue }
            guard (values.fileSize ?? 0) <= 64 * 1024 * 1024 else {
                throw MobileIntegrationError(.invalidConfiguration)
            }
            findings.append(contentsOf: try scan(fileAt: candidate, root: root, exempt: exempt))
        }
        return findings.sorted { left, right in
            left.path == right.path ? left.offset < right.offset : left.path < right.path
        }
    }

    private static func classifyRun(_ value: String, textual: Bool) -> String? {
        if let rule = providerCredentialRule(value) { return rule }
        if isSignedJSONWebToken(value) { return "signed-json-web-token" }
        if textual, isHighEntropyMaterial(value) { return "high-entropy-material" }
        return nil
    }

    private struct PrintableRun {
        let text: String
        let offset: Int
    }

    private static func printableRuns(in data: Data) -> [PrintableRun] {
        var runs: [PrintableRun] = []
        var current: [UInt8] = []
        var start = 0
        var index = 0
        for byte in data {
            if byte >= 0x21 && byte <= 0x7e {
                if current.isEmpty { start = index }
                current.append(byte)
                if current.count > 8192 {
                    appendRun(&runs, current, start)
                    current.removeAll(keepingCapacity: true)
                }
            } else {
                appendRun(&runs, current, start)
                current.removeAll(keepingCapacity: true)
            }
            index += 1
        }
        appendRun(&runs, current, start)
        return runs
    }

    private static func appendRun(_ runs: inout [PrintableRun], _ bytes: [UInt8], _ offset: Int) {
        guard bytes.count >= 16, let text = String(bytes: bytes, encoding: .utf8) else { return }
        runs.append(PrintableRun(text: text, offset: offset))
    }

    private static func isTextualArtifact(_ url: URL) -> Bool {
        let textual: Set<String> = [
            "plist", "json", "xml", "strings", "stringsdict", "yaml", "yml", "txt", "md", "cfg",
            "conf", "ini", "env", "properties", "swift", "h", "m", "mm", "entitlements", "xcconfig",
        ]
        return textual.contains(url.pathExtension.lowercased())
    }

    private static func relativePath(of url: URL, from root: URL) -> String {
        let full = url.standardizedFileURL.path
        let base = root.standardizedFileURL.path
        guard full.hasPrefix(base) else { return full }
        let trimmed = full.dropFirst(base.count)
        return trimmed.hasPrefix("/") ? String(trimmed.dropFirst()) : String(trimmed)
    }

    private static func isBase64Character(_ character: Character) -> Bool {
        guard character.isASCII else { return false }
        return character.isLetter || character.isNumber
            || character == "+" || character == "/" || character == "=" || character == "-" || character == "_"
    }

    private static func isHexCharacter(_ character: Character) -> Bool {
        character.isHexDigit && character.isASCII
    }

    private static func isBase64URLCharacter(_ character: Character) -> Bool {
        character.isASCII && (character.isLetter || character.isNumber || character == "-" || character == "_" || character == "=")
    }

    private static func decodeBase64URL(_ value: String) -> Data? {
        var normalized = value.replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
        while normalized.count % 4 != 0 { normalized.append("=") }
        return Data(base64Encoded: normalized)
    }
}
