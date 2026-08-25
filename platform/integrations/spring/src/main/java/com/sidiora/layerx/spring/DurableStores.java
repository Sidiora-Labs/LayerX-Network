package com.sidiora.layerx.spring;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.node.ObjectNode;
import com.sidiora.layerx.sdk.verify.LocalVerifier;
import java.io.IOException;
import java.io.InputStream;
import java.nio.ByteBuffer;
import java.nio.channels.FileChannel;
import java.nio.channels.FileLock;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.Files;
import java.nio.file.LinkOption;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.nio.file.StandardOpenOption;
import java.nio.file.attribute.PosixFilePermission;
import java.nio.file.attribute.PosixFileAttributeView;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.Arrays;
import java.util.Base64;
import java.util.EnumSet;
import java.util.Set;

/** Durable, process-safe stores for payment fulfillment and webhook replay state. */
public final class DurableStores {
    private static final int MAX_RECORD_BYTES = 4 * 1024 * 1024;
    private static final ObjectMapper MAPPER = new ObjectMapper();
    private static final Set<PosixFilePermission> DIRECTORY_PERMISSIONS = EnumSet.of(
        PosixFilePermission.OWNER_READ, PosixFilePermission.OWNER_WRITE, PosixFilePermission.OWNER_EXECUTE);
    private static final Set<PosixFilePermission> FILE_PERMISSIONS = EnumSet.of(
        PosixFilePermission.OWNER_READ, PosixFilePermission.OWNER_WRITE);

    private DurableStores() {}

    public static Fulfillments.FulfillmentRepository fulfillments(Path root) throws IOException {
        return new FileFulfillmentRepository(prepare(root.resolve("fulfillments")));
    }

    public static Webhooks.DeliveryStore deliveries(Path root) throws IOException {
        return new FileDeliveryStore(prepare(root.resolve("webhooks")));
    }

    private static final class FileFulfillmentRepository implements Fulfillments.FulfillmentRepository {
        private final Path directory;

        private FileFulfillmentRepository(Path directory) { this.directory = directory; }

        @Override
        public Fulfillments.StoredFulfillment fulfill(Fulfillments.ProposedFulfillment proposed,
                                                       ResourceRelease release) throws IOException {
            if (proposed == null || release == null) throw new IOException("invalid fulfillment request");
            return locked(directory, () -> {
                Path path = record(directory, proposed.idempotencyKey());
                if (Files.exists(path, LinkOption.NOFOLLOW_LINKS)) {
                    return readFulfillment(path, proposed);
                }
                LayerXResource resource = release.release();
                Fulfillments.StoredFulfillment stored = new Fulfillments.StoredFulfillment(
                    proposed.idempotencyKey(), proposed.requestDigest(), proposed.canonicalReceipt(),
                    proposed.authorizedBatch(), resource);
                writeAtomic(directory, path, encode(stored));
                return stored;
            });
        }
    }

    private static final class FileDeliveryStore implements Webhooks.DeliveryStore {
        private final Path directory;

        private FileDeliveryStore(Path directory) { this.directory = directory; }

        @Override
        public Webhooks.ClaimResult claim(Webhooks.DeliveryClaim claim) {
            try {
                return locked(directory, () -> {
                    Path path = record(directory, claim.deliveryId());
                    if (!Files.exists(path, LinkOption.NOFOLLOW_LINKS)) {
                        writeAtomic(directory, path, delivery(claim.payloadDigest(), claim.leaseUntilMs(), false));
                        return Webhooks.ClaimResult.CLAIMED;
                    }
                    JsonNode stored = readObject(path);
                    String digest = text(stored, "payloadDigest", 128);
                    if (!digest.equals(claim.payloadDigest())) return Webhooks.ClaimResult.CONFLICT;
                    if (stored.path("completed").asBoolean(false)) return Webhooks.ClaimResult.COMPLETED;
                    long lease = exactLong(stored, "leaseUntilMs");
                    if (lease > System.currentTimeMillis()) return Webhooks.ClaimResult.PROCESSING;
                    writeAtomic(directory, path, delivery(digest, claim.leaseUntilMs(), false));
                    return Webhooks.ClaimResult.CLAIMED;
                });
            } catch (IOException error) {
                throw new IllegalStateException("durable webhook delivery state is unavailable", error);
            }
        }

        @Override
        public void complete(String deliveryId, String payloadDigest) {
            update(deliveryId, payloadDigest, true);
        }

        @Override
        public void release(String deliveryId, String payloadDigest) {
            try {
                locked(directory, () -> {
                    Path path = record(directory, deliveryId);
                    if (!Files.exists(path, LinkOption.NOFOLLOW_LINKS)) return null;
                    JsonNode stored = readObject(path);
                    if (!text(stored, "payloadDigest", 128).equals(payloadDigest)
                            || stored.path("completed").asBoolean(false)) return null;
                    Files.delete(path);
                    syncDirectory(directory);
                    return null;
                });
            } catch (IOException error) {
                throw new IllegalStateException("durable webhook delivery state is unavailable", error);
            }
        }

        private void update(String deliveryId, String payloadDigest, boolean completed) {
            try {
                locked(directory, () -> {
                    Path path = record(directory, deliveryId);
                    if (!Files.exists(path, LinkOption.NOFOLLOW_LINKS)) {
                        throw new IOException("webhook delivery state is missing");
                    }
                    JsonNode stored = readObject(path);
                    if (!text(stored, "payloadDigest", 128).equals(payloadDigest)) {
                        throw new IOException("webhook delivery digest changed");
                    }
                    writeAtomic(directory, path, delivery(payloadDigest, 0L, completed));
                    return null;
                });
            } catch (IOException error) {
                throw new IllegalStateException("durable webhook delivery state is unavailable", error);
            }
        }
    }

    private static Fulfillments.StoredFulfillment readFulfillment(
            Path path, Fulfillments.ProposedFulfillment proposed) throws IOException {
        JsonNode stored = readObject(path);
        String idempotencyKey = text(stored, "idempotencyKey", 256);
        String requestDigest = text(stored, "requestDigest", 256);
        byte[] receipt = bytes(stored, "canonicalReceipt", 1_048_576);
        JsonNode batchNode = object(stored, "authorizedBatch");
        LocalVerifier.AuthorizedReceiptBatch batch = new LocalVerifier.AuthorizedReceiptBatch(
            exactBytes(batchNode, "batchId", 32), exactBytes(batchNode, "asset", 32),
            exactBytes(batchNode, "previousStateRoot", 32), exactBytes(batchNode, "resultingStateRoot", 32),
            exactBytes(batchNode, "sequencerPublicKey", 32));
        JsonNode resourceNode = object(stored, "resource");
        LayerXResource resource = new LayerXResource(text(resourceNode, "contentType", 1024),
            bytes(resourceNode, "body", 2 * 1024 * 1024));
        if (!idempotencyKey.equals(proposed.idempotencyKey())
                || !requestDigest.equals(proposed.requestDigest())
                || !Arrays.equals(receipt, proposed.canonicalReceipt())
                || !same(batch, proposed.authorizedBatch())) {
            throw MiddlewareException.of(MiddlewareException.Code.FULFILLMENT_CONFLICT);
        }
        return new Fulfillments.StoredFulfillment(idempotencyKey, requestDigest, receipt, batch, resource);
    }

    private static byte[] encode(Fulfillments.StoredFulfillment stored) throws IOException {
        ObjectNode root = MAPPER.createObjectNode();
        root.put("idempotencyKey", stored.idempotencyKey());
        root.put("requestDigest", stored.requestDigest());
        root.put("canonicalReceipt", Base64.getEncoder().encodeToString(stored.canonicalReceipt()));
        ObjectNode batch = root.putObject("authorizedBatch");
        batch.put("batchId", Base64.getEncoder().encodeToString(stored.authorizedBatch().batchId()));
        batch.put("asset", Base64.getEncoder().encodeToString(stored.authorizedBatch().asset()));
        batch.put("previousStateRoot", Base64.getEncoder().encodeToString(stored.authorizedBatch().previousStateRoot()));
        batch.put("resultingStateRoot", Base64.getEncoder().encodeToString(stored.authorizedBatch().resultingStateRoot()));
        batch.put("sequencerPublicKey", Base64.getEncoder().encodeToString(stored.authorizedBatch().sequencerPublicKey()));
        ObjectNode resource = root.putObject("resource");
        resource.put("contentType", stored.resource().contentType());
        resource.put("body", Base64.getEncoder().encodeToString(stored.resource().body()));
        return MAPPER.writeValueAsBytes(root);
    }

    private static byte[] delivery(String payloadDigest, long leaseUntilMs, boolean completed) throws IOException {
        ObjectNode root = MAPPER.createObjectNode();
        root.put("payloadDigest", payloadDigest);
        root.put("leaseUntilMs", leaseUntilMs);
        root.put("completed", completed);
        return MAPPER.writeValueAsBytes(root);
    }

    private static Path prepare(Path requested) throws IOException {
        Path directory = requested.toAbsolutePath().normalize();
        Files.createDirectories(directory);
        if (Files.isSymbolicLink(directory) || !Files.isDirectory(directory, LinkOption.NOFOLLOW_LINKS)) {
            throw new IOException("durable store is not a regular directory: " + directory);
        }
        if (Files.getFileAttributeView(directory, PosixFileAttributeView.class,
                LinkOption.NOFOLLOW_LINKS) == null) {
            throw new IOException("durable store requires owner-protected POSIX storage");
        }
        Files.setPosixFilePermissions(directory, DIRECTORY_PERMISSIONS);
        return directory;
    }

    private static Path record(Path directory, String identity) throws IOException {
        if (identity == null || identity.isEmpty() || identity.length() > 512) {
            throw new IOException("durable record identity is invalid");
        }
        return directory.resolve(hex(sha256(identity.getBytes(java.nio.charset.StandardCharsets.UTF_8))) + ".json");
    }

    private interface LockedOperation<T> { T run() throws IOException; }

    private static <T> T locked(Path directory, LockedOperation<T> operation) throws IOException {
        Path lockPath = directory.resolve(".lock");
        if (Files.exists(lockPath, LinkOption.NOFOLLOW_LINKS)
                && (Files.isSymbolicLink(lockPath) || !Files.isRegularFile(lockPath, LinkOption.NOFOLLOW_LINKS))) {
            throw new IOException("durable store lock is not a regular file");
        }
        try (FileChannel channel = FileChannel.open(lockPath, StandardOpenOption.CREATE,
                 StandardOpenOption.READ, StandardOpenOption.WRITE, LinkOption.NOFOLLOW_LINKS);
             FileLock ignored = channel.lock()) {
            permissions(lockPath);
            return operation.run();
        }
    }

    private static void writeAtomic(Path directory, Path path, byte[] encoded) throws IOException {
        if (encoded.length > MAX_RECORD_BYTES) throw new IOException("durable record exceeds its bound");
        Path temporary = Files.createTempFile(directory, ".layerx-", ".tmp");
        boolean published = false;
        try {
            permissions(temporary);
            try (FileChannel output = FileChannel.open(temporary, StandardOpenOption.WRITE,
                     StandardOpenOption.TRUNCATE_EXISTING)) {
                ByteBuffer buffer = ByteBuffer.wrap(encoded);
                while (buffer.hasRemaining()) output.write(buffer);
                output.force(true);
            }
            try {
                Files.move(temporary, path, StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING);
            } catch (AtomicMoveNotSupportedException error) {
                throw new IOException("durable store does not support atomic publication", error);
            }
            published = true;
            permissions(path);
            syncDirectory(directory);
        } finally {
            if (!published) Files.deleteIfExists(temporary);
        }
    }

    private static JsonNode readObject(Path path) throws IOException {
        if (Files.isSymbolicLink(path) || !Files.isRegularFile(path, LinkOption.NOFOLLOW_LINKS)) {
            throw new IOException("durable record is not a regular file");
        }
        byte[] encoded;
        try (InputStream input = Files.newInputStream(path, StandardOpenOption.READ,
                 LinkOption.NOFOLLOW_LINKS)) {
            encoded = input.readNBytes(MAX_RECORD_BYTES + 1);
        }
        if (encoded.length > MAX_RECORD_BYTES) throw new IOException("durable record exceeds its bound");
        JsonNode value = MAPPER.readTree(encoded);
        if (value == null || !value.isObject()) throw new IOException("durable record is not an object");
        return value;
    }

    private static void permissions(Path path) throws IOException {
        if (Files.getFileAttributeView(path, PosixFileAttributeView.class,
                LinkOption.NOFOLLOW_LINKS) == null) {
            throw new IOException("durable store requires owner-protected POSIX storage");
        }
        Files.setPosixFilePermissions(path, FILE_PERMISSIONS);
    }

    private static void syncDirectory(Path directory) throws IOException {
        try (FileChannel channel = FileChannel.open(directory, StandardOpenOption.READ)) { channel.force(true); }
    }

    private static JsonNode object(JsonNode root, String name) throws IOException {
        JsonNode value = root.get(name);
        if (value == null || !value.isObject()) throw new IOException("durable record field is invalid: " + name);
        return value;
    }

    private static String text(JsonNode root, String name, int maximum) throws IOException {
        JsonNode value = root.get(name);
        if (value == null || !value.isTextual() || value.textValue().isEmpty()
                || value.textValue().length() > maximum) {
            throw new IOException("durable record field is invalid: " + name);
        }
        return value.textValue();
    }

    private static byte[] bytes(JsonNode root, String name, int maximum) throws IOException {
        byte[] value;
        try { value = Base64.getDecoder().decode(text(root, name, 3 * maximum)); }
        catch (IllegalArgumentException error) { throw new IOException("durable record field is invalid: " + name, error); }
        if (value.length > maximum) throw new IOException("durable record field is invalid: " + name);
        return value;
    }

    private static byte[] exactBytes(JsonNode root, String name, int length) throws IOException {
        byte[] value = bytes(root, name, length);
        if (value.length != length) throw new IOException("durable record field is invalid: " + name);
        return value;
    }

    private static long exactLong(JsonNode root, String name) throws IOException {
        JsonNode value = root.get(name);
        if (value == null || !value.isIntegralNumber() || !value.canConvertToLong() || value.longValue() < 0) {
            throw new IOException("durable record field is invalid: " + name);
        }
        return value.longValue();
    }

    private static boolean same(LocalVerifier.AuthorizedReceiptBatch left,
                                LocalVerifier.AuthorizedReceiptBatch right) {
        return Arrays.equals(left.batchId(), right.batchId()) && Arrays.equals(left.asset(), right.asset())
            && Arrays.equals(left.previousStateRoot(), right.previousStateRoot())
            && Arrays.equals(left.resultingStateRoot(), right.resultingStateRoot())
            && Arrays.equals(left.sequencerPublicKey(), right.sequencerPublicKey());
    }

    private static byte[] sha256(byte[] value) throws IOException {
        try { return MessageDigest.getInstance("SHA-256").digest(value); }
        catch (NoSuchAlgorithmException error) { throw new IOException("SHA-256 is unavailable", error); }
    }

    private static String hex(byte[] value) { return java.util.HexFormat.of().formatHex(value); }
}
