package com.sidiora.layerx.android;

import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Base64;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import java.util.stream.Stream;
import java.util.zip.ZipEntry;
import java.util.zip.ZipInputStream;

/** Structural detector that keeps API secrets and key material out of shipped mobile artifacts. */
public final class EmbeddedSecretDetector {
    private EmbeddedSecretDetector() {}

    public record Finding(String rule, String path, long offset, int length) {
        @Override public String toString() { return path + ":" + offset + " rule=" + rule + " length=" + length; }
    }

    private static final Map<String, String> PROVIDER_PREFIXES = Map.ofEntries(
        Map.entry("openai-key", "sk-"),
        Map.entry("stripe-secret-key", "sk_live_"),
        Map.entry("stripe-restricted-key", "rk_live_"),
        Map.entry("aws-access-key", "AKIA"),
        Map.entry("aws-temporary-key", "ASIA"),
        Map.entry("github-token", "ghp_"),
        Map.entry("github-oauth-token", "gho_"),
        Map.entry("github-fine-grained-token", "github_pat_"),
        Map.entry("slack-bot-token", "xoxb-"),
        Map.entry("slack-user-token", "xoxp-"),
        Map.entry("google-api-key", "AIza"),
        Map.entry("sendgrid-key", "SG."),
        Map.entry("npm-token", "npm_"),
        Map.entry("gitlab-token", "glpat-"),
        Map.entry("huggingface-token", "hf_"),
        Map.entry("digitalocean-token", "dop_v1_"),
        Map.entry("shopify-token", "shpat_"),
        Map.entry("layerx-service-secret", "lxs_"));

    private static final Map<String, String> PEM_MARKERS = Map.of(
        "pem-private-key", "-----BEGIN PRIVATE KEY-----",
        "pem-rsa-private-key", "-----BEGIN RSA PRIVATE KEY-----",
        "pem-ec-private-key", "-----BEGIN EC PRIVATE KEY-----",
        "pem-encrypted-private-key", "-----BEGIN ENCRYPTED PRIVATE KEY-----",
        "openssh-private-key", "-----BEGIN OPENSSH PRIVATE KEY-----",
        "pgp-private-key", "-----BEGIN PGP PRIVATE KEY BLOCK-----");

    private static final List<String> SECRET_NAMES = List.of(
        "secret", "api_key", "apikey", "api-key", "private_key", "privatekey", "private-key",
        "password", "passphrase", "client_secret", "access_token", "refresh_token", "bearer",
        "signing_key", "seed", "mnemonic", "credential", "authorization");

    private static final Set<String> TEXTUAL_EXTENSIONS = Set.of(
        "json", "xml", "properties", "yaml", "yml", "txt", "md", "cfg", "conf", "ini", "env",
        "java", "kt", "kts", "gradle", "pro", "cfgxml", "arsc", "smali");

    private static final int MINIMUM_RUN = 16;
    private static final int MAXIMUM_RUN = 8192;
    private static final long MAXIMUM_FILE_BYTES = 64L * 1024L * 1024L;
    private static final int MAXIMUM_ARCHIVE_ENTRIES = 100_000;

    public static boolean isSecretShapedName(String name) {
        String normalized = name.toLowerCase(Locale.ROOT);
        for (String candidate : SECRET_NAMES) {
            if (normalized.contains(candidate)) return true;
        }
        return false;
    }

    public static String providerCredentialRule(String value) {
        for (Map.Entry<String, String> marker : PEM_MARKERS.entrySet()) {
            if (value.contains(marker.getValue())) return marker.getKey();
        }
        for (Map.Entry<String, String> prefix : PROVIDER_PREFIXES.entrySet()) {
            if (value.startsWith(prefix.getValue()) && value.length() >= prefix.getValue().length() + 12) {
                return prefix.getKey();
            }
        }
        return null;
    }

    public static String classify(String value) {
        String provider = providerCredentialRule(value);
        if (provider != null) return provider;
        if (isSignedJsonWebToken(value)) return "signed-json-web-token";
        if (isHighEntropyMaterial(value)) return "high-entropy-material";
        return null;
    }

    public static boolean isSignedJsonWebToken(String value) {
        String[] segments = value.split("\\.", -1);
        if (segments.length != 3 || segments[0].length() < 8 || segments[1].length() < 8 || segments[2].length() < 16) {
            return false;
        }
        for (String segment : segments) {
            for (int index = 0; index < segment.length(); index++) {
                if (!isBase64UrlCharacter(segment.charAt(index))) return false;
            }
        }
        try {
            String header = new String(Base64.getUrlDecoder().decode(padded(segments[0])), StandardCharsets.UTF_8);
            return header.contains("\"alg\"");
        } catch (IllegalArgumentException error) {
            return false;
        }
    }

    public static boolean isHighEntropyMaterial(String value) {
        if (value.length() < 40 || value.length() > 4096) return false;
        boolean base64Like = true;
        boolean hexLike = true;
        for (int index = 0; index < value.length(); index++) {
            char character = value.charAt(index);
            if (!isBase64Character(character)) base64Like = false;
            if (!isHexCharacter(character)) hexLike = false;
        }
        if (!base64Like && !hexLike) return false;
        if (hexLike && value.length() <= 64) return false;
        return shannonEntropyBitsPerCharacter(value.getBytes(StandardCharsets.UTF_8)) >= 3.6d;
    }

    public static double shannonEntropyBitsPerCharacter(byte[] bytes) {
        if (bytes.length == 0) return 0d;
        int[] counts = new int[256];
        for (byte value : bytes) counts[value & 0xff]++;
        double total = bytes.length;
        double entropy = 0d;
        for (int count : counts) {
            if (count == 0) continue;
            double probability = count / total;
            entropy -= probability * (Math.log(probability) / Math.log(2d));
        }
        return entropy;
    }

    public static List<Finding> scan(InputStream stream, String path, boolean textual, Set<String> exempt)
            throws IOException {
        List<Finding> findings = new ArrayList<>();
        StringBuilder current = new StringBuilder();
        long offset = 0;
        long runStart = 0;
        int value;
        while ((value = stream.read()) >= 0) {
            if (value >= 0x21 && value <= 0x7e) {
                if (current.isEmpty()) runStart = offset;
                current.append((char) value);
                if (current.length() >= MAXIMUM_RUN) {
                    appendRun(findings, current, runStart, path, textual, exempt);
                }
            } else {
                appendRun(findings, current, runStart, path, textual, exempt);
            }
            offset++;
        }
        appendRun(findings, current, runStart, path, textual, exempt);
        return findings;
    }

    public static List<Finding> scanArtifact(Path root, Set<String> exempt) throws IOException {
        if (!Files.exists(root)) throw MobileIntegrationException.of(MobileIntegrationException.Code.INVALID_CONFIGURATION);
        if (!Files.isDirectory(root)) {
            if (isArchive(root)) return scanArchive(root, exempt);
            return scanFile(root, root.getFileName().toString(), exempt);
        }
        List<Finding> findings = new ArrayList<>();
        try (Stream<Path> walker = Files.walk(root)) {
            List<Path> files = walker.filter(Files::isRegularFile).sorted().toList();
            for (Path file : files) {
                if (Files.size(file) > MAXIMUM_FILE_BYTES) {
                    throw MobileIntegrationException.of(MobileIntegrationException.Code.INVALID_CONFIGURATION);
                }
                findings.addAll(isArchive(file)
                    ? scanArchive(file, exempt)
                    : scanFile(file, root.relativize(file).toString(), exempt));
            }
        }
        return findings;
    }

    private static List<Finding> scanFile(Path file, String path, Set<String> exempt) throws IOException {
        try (InputStream stream = new java.io.BufferedInputStream(Files.newInputStream(file))) {
            return scan(stream, path, isTextual(file), exempt);
        }
    }

    private static List<Finding> scanArchive(Path file, Set<String> exempt) throws IOException {
        List<Finding> findings = new ArrayList<>();
        try (ZipInputStream archive = new ZipInputStream(
                new java.io.BufferedInputStream(Files.newInputStream(file)))) {
            int entries = 0;
            ZipEntry entry;
            while ((entry = archive.getNextEntry()) != null) {
                entries++;
                if (entries > MAXIMUM_ARCHIVE_ENTRIES || entry.getName().indexOf('\0') >= 0) {
                    throw MobileIntegrationException.of(MobileIntegrationException.Code.INVALID_CONFIGURATION);
                }
                if (!entry.isDirectory()) {
                    String path = file.getFileName() + "!/" + entry.getName();
                    findings.addAll(scan(new BoundedInputStream(archive, MAXIMUM_FILE_BYTES), path,
                        isTextual(entry.getName()), exempt));
                }
                archive.closeEntry();
            }
        }
        return findings;
    }

    private static final class BoundedInputStream extends InputStream {
        private final InputStream source;
        private final long maximum;
        private long consumed;

        private BoundedInputStream(InputStream source, long maximum) {
            this.source = source;
            this.maximum = maximum;
        }

        @Override public int read() throws IOException {
            int value = source.read();
            if (value >= 0 && ++consumed > maximum) throw new IOException("archive entry exceeds scan bound");
            return value;
        }
    }

    private static void appendRun(List<Finding> findings, StringBuilder current, long runStart, String path,
                                  boolean textual, Set<String> exempt) {
        if (current.length() >= MINIMUM_RUN) {
            String text = current.toString();
            if (!exempt.contains(text)) {
                String rule = classifyRun(text, textual);
                if (rule != null) findings.add(new Finding(rule, path, runStart, text.length()));
            }
        }
        current.setLength(0);
    }

    private static String classifyRun(String value, boolean textual) {
        String provider = providerCredentialRule(value);
        if (provider != null) return provider;
        if (isSignedJsonWebToken(value)) return "signed-json-web-token";
        if (textual && isHighEntropyMaterial(value)) return "high-entropy-material";
        return null;
    }

    private static boolean isTextual(Path file) {
        return isTextual(file.getFileName().toString());
    }

    private static boolean isTextual(String name) {
        int dot = name.lastIndexOf('.');
        if (dot < 0) return false;
        return TEXTUAL_EXTENSIONS.contains(name.substring(dot + 1).toLowerCase(Locale.ROOT));
    }

    private static boolean isArchive(Path file) {
        String name = file.getFileName().toString().toLowerCase(Locale.ROOT);
        return name.endsWith(".apk") || name.endsWith(".aab") || name.endsWith(".jar") || name.endsWith(".zip");
    }

    private static String padded(String segment) {
        StringBuilder builder = new StringBuilder(segment);
        while (builder.length() % 4 != 0) builder.append('=');
        return builder.toString();
    }

    private static boolean isBase64Character(char character) {
        return (character >= 'A' && character <= 'Z') || (character >= 'a' && character <= 'z')
            || (character >= '0' && character <= '9')
            || character == '+' || character == '/' || character == '=' || character == '-' || character == '_';
    }

    private static boolean isBase64UrlCharacter(char character) {
        return (character >= 'A' && character <= 'Z') || (character >= 'a' && character <= 'z')
            || (character >= '0' && character <= '9') || character == '-' || character == '_' || character == '=';
    }

    private static boolean isHexCharacter(char character) {
        return (character >= '0' && character <= '9') || (character >= 'a' && character <= 'f')
            || (character >= 'A' && character <= 'F');
    }
}
