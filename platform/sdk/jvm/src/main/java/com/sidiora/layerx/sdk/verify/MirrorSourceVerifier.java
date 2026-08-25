package com.sidiora.layerx.sdk.verify;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.node.ArrayNode;
import com.fasterxml.jackson.databind.node.ObjectNode;
import com.sidiora.layerx.sdk.GeneratedMirror;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.math.BigInteger;
import java.nio.file.Files;
import java.nio.file.LinkOption;
import java.nio.file.Path;
import java.nio.file.attribute.BasicFileAttributes;
import java.nio.file.attribute.PosixFilePermission;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.time.Duration;
import java.util.HashSet;
import java.util.HexFormat;
import java.util.List;
import java.util.Set;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;

public final class MirrorSourceVerifier {
    public record Candidate(int source, byte[] commitment) {}
    public record Policy(GeneratedMirror.Policy kind, List<Candidate> candidates, int minimum) {}
    public record Verification(String level, BigInteger batchNumber, byte[] headerDigest,
        byte[] evidenceDigest, String sourceId, String target, String canonicalPosition,
        String provenance, BigInteger latestBatch, String batchLag, int failoverCount,
        int agreeingSources, String checkpointLevel) {}
    public static final class VerificationException extends Exception {
        private final String code;
        public VerificationException(String code) {
            super("mirror verification refused: " + code);
            this.code = code;
        }
        public String code() { return code; }
    }

    private static final int MAX_REQUEST = 40 * 1024 * 1024;
    private static final int MAX_RESPONSE = 1024 * 1024;
    private static final int MAX_EVIDENCE = (MAX_REQUEST - 64 * 1024) / 2;
    private static final long MAX_EXECUTABLE = 512L * 1024 * 1024;
    private static final long MAX_CONFIGURATION = 16L * 1024 * 1024;
    private static final String CURRENT_OWNER = System.getProperty("user.name", "");
    private static final Set<String> ERRORS = Set.of(
        "configuration", "unavailable", "rate-limited", "missing", "target-mismatch",
        "source-mismatch", "malformed", "bounds", "commitment", "authorization", "proof",
        "checkpoint-unavailable", "divergent", "insufficient-agreement", "reorged");

    private final Path executable;
    private final Path configuration;
    private final byte[] executableDigest;
    private final byte[] configurationDigest;
    private final Duration timeout;
    private final ObjectMapper json = new ObjectMapper();

    public MirrorSourceVerifier(Path executable, Path configuration, Duration timeout)
        throws VerificationException {
        if (timeout.compareTo(Duration.ofMillis(100)) < 0
            || timeout.compareTo(Duration.ofSeconds(120)) > 0) {
            throw new VerificationException("configuration");
        }
        TrustedInput executableInput = trustedInput(executable, true, MAX_EXECUTABLE);
        TrustedInput configurationInput = trustedInput(configuration, false, MAX_CONFIGURATION);
        this.executable = executableInput.path();
        this.configuration = configurationInput.path();
        this.executableDigest = executableInput.digest();
        this.configurationDigest = configurationInput.digest();
        this.timeout = timeout;
    }

    public Verification receipt(BigInteger batch, Policy policy, byte[] receipt)
        throws VerificationException {
        if (receipt == null || receipt.length > MAX_EVIDENCE) {
            throw new VerificationException("bounds");
        }
        ObjectNode evidence = json.createObjectNode().put("kind", "receipt")
            .put("canonical_hex", HexFormat.of().formatHex(receipt));
        return verify(batch, policy, evidence);
    }

    public Verification state(BigInteger batch, Policy policy, byte[] state, byte[] proof)
        throws VerificationException {
        if (state == null || proof == null || state.length > MAX_EVIDENCE
            || proof.length > MAX_EVIDENCE - state.length) {
            throw new VerificationException("bounds");
        }
        ObjectNode evidence = json.createObjectNode().put("kind", "state")
            .put("canonical_hex", HexFormat.of().formatHex(state))
            .put("proof_hex", HexFormat.of().formatHex(proof));
        return verify(batch, policy, evidence);
    }

    private Verification verify(BigInteger batch, Policy policy, ObjectNode evidence)
        throws VerificationException {
        if (batch == null || policy == null || policy.candidates() == null
            || policy.kind() == null || batch.signum() <= 0 || batch.bitLength() > 64
            || policy.candidates().isEmpty()
            || policy.candidates().size() > GeneratedMirror.MAX_SOURCES) {
            throw new VerificationException("configuration");
        }
        ArrayNode candidates = json.createArrayNode();
        HashSet<Integer> seen = new HashSet<>();
        for (Candidate value : policy.candidates()) {
            if (value == null || value.source() < 0 || !seen.add(value.source())
                || value.commitment() == null || value.commitment().length != 32) {
                throw new VerificationException("configuration");
            }
            candidates.addObject().put("source", value.source())
                .put("commitment_hex", HexFormat.of().formatHex(value.commitment()));
        }
        ObjectNode wirePolicy = json.createObjectNode();
        switch (policy.kind()) {
            case EXACT -> {
                if (candidates.size() != 1) throw new VerificationException("configuration");
                wirePolicy.put("kind", "exact").set("candidate", candidates.get(0));
            }
            case ORDERED_PREFERENCE ->
                wirePolicy.put("kind", "ordered-preference").set("candidates", candidates);
            case AGREEMENT -> {
                if (policy.minimum() < 1 || policy.minimum() > candidates.size()) {
                    throw new VerificationException("configuration");
                }
                wirePolicy.put("kind", "agreement").put("minimum", policy.minimum())
                    .set("candidates", candidates);
            }
            default -> throw new VerificationException("configuration");
        }
        ObjectNode request = json.createObjectNode().put("batch_number", batch.toString())
            .set("evidence", evidence);
        request.set("policy", wirePolicy);
        Process process = null;
        try {
            byte[] bytes = json.writeValueAsBytes(request);
            if (bytes.length > MAX_REQUEST) throw new VerificationException("bounds");
            requireTrustedInputs();
            process = new ProcessBuilder(executable.toString(), configuration.toString())
                .redirectError(ProcessBuilder.Redirect.DISCARD).start();
            Process running = process;
            CompletableFuture<Void> input = CompletableFuture.runAsync(() -> write(running, bytes));
            CompletableFuture<BoundedOutput> output = CompletableFuture.supplyAsync(
                () -> read(running));
            CompletableFuture.allOf(input, output, process.onExit()).get(
                timeout.toMillis(), TimeUnit.MILLISECONDS);
            BoundedOutput responseBytes = output.get();
            requireTrustedInputs();
            if (process.exitValue() != 0) throw new VerificationException("unavailable");
            if (responseBytes.exceeded()) throw new VerificationException("bounds");
            return parse(json.readTree(responseBytes.bytes()), batch, policy);
        } catch (InterruptedException error) {
            Thread.currentThread().interrupt();
            throw new VerificationException("unavailable");
        } catch (TimeoutException | ExecutionException | IOException error) {
            throw new VerificationException("unavailable");
        } finally {
            if (process != null && process.isAlive()) process.destroyForcibly();
        }
    }

    private record TrustedInput(Path path, byte[] digest) {}

    private static TrustedInput trustedInput(Path path, boolean executable, long maximum)
        throws VerificationException {
        try {
            if (path == null || !path.isAbsolute() || !path.normalize().equals(path)) {
                throw new VerificationException("configuration");
            }
            Path current = path;
            while (current != null) {
                if (Files.isSymbolicLink(current)) throw new VerificationException("configuration");
                if (!current.equals(path) && !Files.isDirectory(current, LinkOption.NOFOLLOW_LINKS)) {
                    throw new VerificationException("configuration");
                }
                String owner = Files.getOwner(current, LinkOption.NOFOLLOW_LINKS).getName();
                if (!owner.equals("root") && !owner.equals(CURRENT_OWNER)
                    && !owner.endsWith("\\" + CURRENT_OWNER)
                    && !owner.endsWith("/" + CURRENT_OWNER)) {
                    throw new VerificationException("configuration");
                }
                try {
                    Set<PosixFilePermission> permissions = Files.getPosixFilePermissions(
                        current, LinkOption.NOFOLLOW_LINKS);
                    if (permissions.contains(PosixFilePermission.GROUP_WRITE)
                        || permissions.contains(PosixFilePermission.OTHERS_WRITE)) {
                        throw new VerificationException("configuration");
                    }
                } catch (UnsupportedOperationException ignored) {
                    // The platform exposes no POSIX modes; link and identity checks remain mandatory.
                }
                current = current.getParent();
            }
            if (!Files.isRegularFile(path, LinkOption.NOFOLLOW_LINKS)
                || (executable && !Files.isExecutable(path))) {
                throw new VerificationException("configuration");
            }
            BasicFileAttributes before = Files.readAttributes(
                path, BasicFileAttributes.class, LinkOption.NOFOLLOW_LINKS);
            if (before.size() < 0 || before.size() > maximum) {
                throw new VerificationException("configuration");
            }
            MessageDigest digest = MessageDigest.getInstance("SHA-256");
            long total = 0;
            try (InputStream source = Files.newInputStream(path, LinkOption.NOFOLLOW_LINKS)) {
                byte[] buffer = new byte[64 * 1024];
                for (int count; (count = source.read(buffer)) >= 0;) {
                    total += count;
                    if (total > maximum) throw new VerificationException("configuration");
                    digest.update(buffer, 0, count);
                }
            }
            BasicFileAttributes after = Files.readAttributes(
                path, BasicFileAttributes.class, LinkOption.NOFOLLOW_LINKS);
            if (total != before.size() || !sameFile(before, after)) {
                throw new VerificationException("configuration");
            }
            return new TrustedInput(path, digest.digest());
        } catch (IOException | NoSuchAlgorithmException error) {
            throw new VerificationException("configuration");
        }
    }

    private static boolean sameFile(BasicFileAttributes left, BasicFileAttributes right) {
        Object leftKey = left.fileKey();
        Object rightKey = right.fileKey();
        return left.size() == right.size()
            && left.lastModifiedTime().equals(right.lastModifiedTime())
            && (leftKey == null ? rightKey == null : leftKey.equals(rightKey));
    }

    private void requireTrustedInputs() throws VerificationException {
        TrustedInput executableInput = trustedInput(executable, true, MAX_EXECUTABLE);
        TrustedInput configurationInput = trustedInput(configuration, false, MAX_CONFIGURATION);
        if (!MessageDigest.isEqual(executableDigest, executableInput.digest())
            || !MessageDigest.isEqual(configurationDigest, configurationInput.digest())) {
            throw new VerificationException("configuration");
        }
    }

    private Verification parse(JsonNode response, BigInteger requestedBatch, Policy policy)
        throws VerificationException {
        if (response == null || !response.path("ok").asBoolean(false)) {
            String code = response == null ? "malformed" : response.path("error").asText("malformed");
            throw new VerificationException(ERRORS.contains(code) ? code : "malformed");
        }
        JsonNode value = required(response, "verification");
        BigInteger batch = unsignedText(required(value, "batchNumber"));
        String level = text(value, "level", 64);
        String source = text(value, "sourceId", 64);
        String target = text(value, "target", 2048);
        String position = text(value, "canonicalPosition", 2048);
        String provenance = text(value, "provenance", 16);
        String lag = text(value, "batchLag", 64);
        String checkpoint = text(value, "checkpointLevel", 32);
        int failover = integer(value, "failoverCount");
        int agreeing = integer(value, "agreeingSources");
        if (!batch.equals(requestedBatch)
            || !(provenance.equals("Canonical") || provenance.equals("Reorged"))
            || !checkpoint.equals("unavailable") || failover < 0
            || failover >= policy.candidates().size() || agreeing < 1
            || agreeing > policy.candidates().size()
            || (policy.kind() == GeneratedMirror.Policy.AGREEMENT && agreeing < policy.minimum())) {
            throw new VerificationException("malformed");
        }
        JsonNode latestNode = value.get("latestBatch");
        BigInteger latest = latestNode == null || latestNode.isNull() ? null : unsignedText(latestNode);
        return new Verification(level, batch, digest(text(value, "headerDigest", 64)),
            digest(text(value, "evidenceDigest", 64)), source, target, position, provenance,
            latest, lag, failover, agreeing, checkpoint);
    }

    private static void write(Process process, byte[] bytes) {
        try (var input = process.getOutputStream()) {
            input.write(bytes);
        } catch (IOException error) {
            throw new IllegalStateException(error);
        }
    }

    private static BoundedOutput read(Process process) {
        try (var input = process.getInputStream(); var output = new ByteArrayOutputStream()) {
            byte[] chunk = new byte[8192];
            boolean exceeded = false;
            int count;
            while ((count = input.read(chunk)) != -1) {
                int keep = Math.min(Math.max(MAX_RESPONSE - output.size(), 0), count);
                if (keep != 0) output.write(chunk, 0, keep);
                if (keep != count) exceeded = true;
            }
            return new BoundedOutput(output.toByteArray(), exceeded);
        } catch (IOException error) {
            throw new IllegalStateException(error);
        }
    }

    private static JsonNode required(JsonNode value, String field) throws VerificationException {
        JsonNode result = value.get(field);
        if (result == null || result.isNull()) throw new VerificationException("malformed");
        return result;
    }

    private static String text(JsonNode value, String field, int maximum)
        throws VerificationException {
        JsonNode result = required(value, field);
        if (!result.isTextual() || result.textValue().isEmpty()
            || result.textValue().getBytes(StandardCharsets.UTF_8).length > maximum) {
            throw new VerificationException("malformed");
        }
        return result.textValue();
    }

    private static int integer(JsonNode value, String field) throws VerificationException {
        JsonNode result = required(value, field);
        if (!result.isInt()) throw new VerificationException("malformed");
        return result.intValue();
    }

    private static byte[] digest(String value) throws VerificationException {
        try {
            byte[] result = HexFormat.of().parseHex(value);
            if (result.length != 32) throw new VerificationException("malformed");
            return result;
        } catch (IllegalArgumentException error) {
            throw new VerificationException("malformed");
        }
    }

    private static BigInteger unsignedText(JsonNode value) throws VerificationException {
        if (!value.isTextual() || !value.textValue().matches("[1-9][0-9]*")) {
            throw new VerificationException("malformed");
        }
        BigInteger result = new BigInteger(value.textValue());
        if (result.bitLength() > 64) throw new VerificationException("malformed");
        return result;
    }

    private record BoundedOutput(byte[] bytes, boolean exceeded) {}
}
