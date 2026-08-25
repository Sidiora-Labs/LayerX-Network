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
import java.nio.file.FileAlreadyExistsException;
import java.nio.file.Files;
import java.nio.file.LinkOption;
import java.nio.file.NoSuchFileException;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.nio.file.StandardOpenOption;
import java.nio.file.attribute.PosixFilePermission;
import java.nio.file.attribute.PosixFileAttributeView;
import java.nio.file.attribute.PosixFilePermissions;
import java.nio.file.attribute.BasicFileAttributes;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Base64;
import java.util.EnumSet;
import java.util.List;
import java.util.Set;

/** Durable, process-safe stores for payment fulfillment and webhook replay state. */
public final class DurableStores {
    private static final int MAX_RECORD_BYTES = 4 * 1024 * 1024;
    private static final ObjectMapper MAPPER = new ObjectMapper();
    private static final Set<PosixFilePermission> DIRECTORY_PERMISSIONS = EnumSet.of(
        PosixFilePermission.OWNER_READ, PosixFilePermission.OWNER_WRITE, PosixFilePermission.OWNER_EXECUTE);
    private static final Set<PosixFilePermission> FILE_PERMISSIONS = EnumSet.of(
        PosixFilePermission.OWNER_READ, PosixFilePermission.OWNER_WRITE);
    private static final String CURRENT_OWNER = currentOwner();

    private DurableStores() {}

    public static Fulfillments.FulfillmentRepository fulfillments(Path root) throws IOException {
        return new FileFulfillmentRepository(prepare(root.resolve("fulfillments")));
    }

    public static Webhooks.DeliveryStore deliveries(Path root) throws IOException {
        return new FileDeliveryStore(prepare(root.resolve("webhooks")));
    }

    private record DirectoryIdentity(Path path, Object fileKey, String owner,
                                     Set<PosixFilePermission> permissions) {}

    private record TrustedDirectory(Path path, List<DirectoryIdentity> ancestors) {}

    private record TrustedFile(Object fileKey, String owner, Set<PosixFilePermission> permissions,
                               long size) {}

    private static final class FileFulfillmentRepository implements Fulfillments.FulfillmentRepository {
        private final TrustedDirectory directory;

        private FileFulfillmentRepository(TrustedDirectory directory) { this.directory = directory; }

        @Override
        public Fulfillments.StoredFulfillment fulfill(Fulfillments.ProposedFulfillment proposed,
                                                       ResourceRelease release) throws IOException {
            if (proposed == null || release == null) throw new IOException("invalid fulfillment request");
            return locked(directory, () -> {
                Path path = record(directory.path(), proposed.idempotencyKey());
                if (Files.exists(path, LinkOption.NOFOLLOW_LINKS)) {
                    return readFulfillment(directory, path, proposed);
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
        private final TrustedDirectory directory;

        private FileDeliveryStore(TrustedDirectory directory) { this.directory = directory; }

        @Override
        public Webhooks.ClaimResult claim(Webhooks.DeliveryClaim claim) {
            try {
                return locked(directory, () -> {
                    Path path = record(directory.path(), claim.deliveryId());
                    if (!Files.exists(path, LinkOption.NOFOLLOW_LINKS)) {
                        writeAtomic(directory, path, delivery(claim.payloadDigest(), claim.leaseUntilMs(), false));
                        return Webhooks.ClaimResult.CLAIMED;
                    }
                    JsonNode stored = readObject(directory, path);
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
                    Path path = record(directory.path(), deliveryId);
                    if (!Files.exists(path, LinkOption.NOFOLLOW_LINKS)) return null;
                    JsonNode stored = readObject(directory, path);
                    if (!text(stored, "payloadDigest", 128).equals(payloadDigest)
                            || stored.path("completed").asBoolean(false)) return null;
                    TrustedFile deleting = trustedFile(path);
                    if (!sameFile(deleting, trustedFile(path))) {
                        throw new IOException("durable record changed before deletion");
                    }
                    Files.delete(path);
                    requireTrustedDirectory(directory);
                    if (Files.exists(path, LinkOption.NOFOLLOW_LINKS)) {
                        throw new IOException("durable record remained after deletion");
                    }
                    syncDirectory(directory.path());
                    return null;
                });
            } catch (IOException error) {
                throw new IllegalStateException("durable webhook delivery state is unavailable", error);
            }
        }

        private void update(String deliveryId, String payloadDigest, boolean completed) {
            try {
                locked(directory, () -> {
                    Path path = record(directory.path(), deliveryId);
                    if (!Files.exists(path, LinkOption.NOFOLLOW_LINKS)) {
                        throw new IOException("webhook delivery state is missing");
                    }
                    JsonNode stored = readObject(directory, path);
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
            TrustedDirectory directory, Path path,
            Fulfillments.ProposedFulfillment proposed) throws IOException {
        JsonNode stored = readObject(directory, path);
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

    private static TrustedDirectory prepare(Path requested) throws IOException {
        if (!requested.isAbsolute() || !requested.normalize().equals(requested)
                || requested.getRoot() == null) {
            throw new IOException("durable store path must be absolute and canonical");
        }
        Path directory = requested;
        Path current = directory.getRoot();
        inspectDirectory(current, false);
        for (Path component : current.relativize(directory)) {
            Path next = current.resolve(component);
            try {
                inspectDirectory(next, next.equals(directory));
            } catch (NoSuchFileException missing) {
                DirectoryIdentity parentBefore = inspectDirectory(current, false);
                try {
                    Files.createDirectory(next,
                        PosixFilePermissions.asFileAttribute(DIRECTORY_PERMISSIONS));
                } catch (FileAlreadyExistsException raced) {}
                DirectoryIdentity parentAfter = inspectDirectory(current, false);
                if (!sameDirectory(parentBefore, parentAfter)) {
                    throw new IOException("durable store parent changed during creation: " + current);
                }
                DirectoryIdentity created = inspectDirectory(next, true);
                if (!sameDirectory(created, inspectDirectory(next, true))) {
                    throw new IOException("durable store owner changed during creation: " + next);
                }
            }
            current = next;
        }
        TrustedDirectory trusted = captureTrustedDirectory(directory);
        requireTrustedDirectory(trusted);
        return trusted;
    }

    private static Path record(Path directory, String identity) throws IOException {
        if (identity == null || identity.isEmpty() || identity.length() > 512) {
            throw new IOException("durable record identity is invalid");
        }
        return directory.resolve(hex(sha256(identity.getBytes(java.nio.charset.StandardCharsets.UTF_8))) + ".json");
    }

    private interface LockedOperation<T> { T run() throws IOException; }

    private static <T> T locked(TrustedDirectory directory, LockedOperation<T> operation)
            throws IOException {
        requireTrustedDirectory(directory);
        Path lockPath = directory.path().resolve(".lock");
        TrustedFile before = optionalTrustedFile(lockPath);
        try (FileChannel channel = FileChannel.open(lockPath,
                 Set.of(StandardOpenOption.CREATE, StandardOpenOption.READ,
                    StandardOpenOption.WRITE, LinkOption.NOFOLLOW_LINKS),
                 PosixFilePermissions.asFileAttribute(FILE_PERMISSIONS));
             FileLock ignored = channel.lock()) {
            permissions(lockPath);
            TrustedFile opened = trustedFile(lockPath);
            if (before != null && !sameFile(before, opened)) {
                throw new IOException("durable store lock changed while it was opened");
            }
            requireTrustedDirectory(directory);
            T result = operation.run();
            requireTrustedDirectory(directory);
            if (!sameFile(opened, trustedFile(lockPath))) {
                throw new IOException("durable store lock changed while held");
            }
            return result;
        }
    }

    private static void writeAtomic(TrustedDirectory directory, Path path, byte[] encoded)
            throws IOException {
        if (encoded.length > MAX_RECORD_BYTES) throw new IOException("durable record exceeds its bound");
        requireTrustedDirectory(directory);
        TrustedFile before = optionalTrustedFile(path);
        Path temporary = Files.createTempFile(directory.path(), ".layerx-", ".tmp",
            PosixFilePermissions.asFileAttribute(FILE_PERMISSIONS));
        boolean published = false;
        try {
            permissions(temporary);
            try (FileChannel output = FileChannel.open(temporary, StandardOpenOption.WRITE,
                     StandardOpenOption.TRUNCATE_EXISTING)) {
                ByteBuffer buffer = ByteBuffer.wrap(encoded);
                while (buffer.hasRemaining()) output.write(buffer);
                output.force(true);
            }
            TrustedFile temporaryIdentity = trustedFile(temporary);
            requireTrustedDirectory(directory);
            TrustedFile current = optionalTrustedFile(path);
            if (!sameOptionalFile(before, current)) {
                throw new IOException("durable record changed while publication was prepared");
            }
            try {
                Files.move(temporary, path, StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING);
            } catch (AtomicMoveNotSupportedException error) {
                throw new IOException("durable store does not support atomic publication", error);
            }
            published = true;
            if (!sameFile(temporaryIdentity, trustedFile(path))) {
                throw new IOException("durable record identity changed during publication");
            }
            requireTrustedDirectory(directory);
            syncDirectory(directory.path());
        } finally {
            if (!published) Files.deleteIfExists(temporary);
        }
    }

    private static JsonNode readObject(TrustedDirectory directory, Path path) throws IOException {
        requireTrustedDirectory(directory);
        TrustedFile before = trustedFile(path);
        if (before.size() > MAX_RECORD_BYTES) throw new IOException("durable record exceeds its bound");
        byte[] encoded;
        try (InputStream input = Files.newInputStream(path, StandardOpenOption.READ,
                 LinkOption.NOFOLLOW_LINKS)) {
            encoded = input.readNBytes(MAX_RECORD_BYTES + 1);
        }
        if (encoded.length > MAX_RECORD_BYTES) throw new IOException("durable record exceeds its bound");
        TrustedFile after = trustedFile(path);
        if (!sameFile(before, after) || after.size() != encoded.length) {
            throw new IOException("durable record changed while it was read");
        }
        requireTrustedDirectory(directory);
        JsonNode value = MAPPER.readTree(encoded);
        if (value == null || !value.isObject()) throw new IOException("durable record is not an object");
        return value;
    }

    private static void permissions(Path path) throws IOException {
        PosixFileAttributeView view = Files.getFileAttributeView(
            path, PosixFileAttributeView.class, LinkOption.NOFOLLOW_LINKS);
        if (view == null) {
            throw new IOException("durable store requires owner-protected POSIX storage");
        }
        view.setPermissions(FILE_PERMISSIONS);
        trustedFile(path);
    }

    private static DirectoryIdentity inspectDirectory(Path path, boolean requireCurrentOwner)
            throws IOException {
        BasicFileAttributes attributes = Files.readAttributes(
            path, BasicFileAttributes.class, LinkOption.NOFOLLOW_LINKS);
        if (!attributes.isDirectory() || Files.isSymbolicLink(path)
                || attributes.fileKey() == null) {
            throw new IOException("durable store ancestor is not a stable directory: " + path);
        }
        PosixFileAttributeView view = Files.getFileAttributeView(
            path, PosixFileAttributeView.class, LinkOption.NOFOLLOW_LINKS);
        if (view == null) throw new IOException("durable store requires POSIX storage: " + path);
        Set<PosixFilePermission> modes = Files.getPosixFilePermissions(
            path, LinkOption.NOFOLLOW_LINKS);
        if (modes.contains(PosixFilePermission.GROUP_WRITE)
                || modes.contains(PosixFilePermission.OTHERS_WRITE)) {
            throw new IOException("durable store ancestor is group/world writable: " + path);
        }
        if (requireCurrentOwner && (modes.contains(PosixFilePermission.GROUP_READ)
                || modes.contains(PosixFilePermission.GROUP_EXECUTE)
                || modes.contains(PosixFilePermission.OTHERS_READ)
                || modes.contains(PosixFilePermission.OTHERS_EXECUTE))) {
            throw new IOException("durable store directory is accessible beyond its owner: " + path);
        }
        String pathOwner = owner(path);
        if (!ownerAllowed(pathOwner) || (requireCurrentOwner && !ownerIsCurrent(pathOwner))) {
            throw new IOException("durable store ancestor has an unsafe owner: " + path);
        }
        return new DirectoryIdentity(path, attributes.fileKey(), pathOwner, Set.copyOf(modes));
    }

    private static TrustedDirectory captureTrustedDirectory(Path directory) throws IOException {
        List<DirectoryIdentity> ancestors = new ArrayList<>();
        Path current = directory.getRoot();
        ancestors.add(inspectDirectory(current, false));
        for (Path component : current.relativize(directory)) {
            current = current.resolve(component);
            ancestors.add(inspectDirectory(current, current.equals(directory)));
        }
        return new TrustedDirectory(directory, List.copyOf(ancestors));
    }

    private static void requireTrustedDirectory(TrustedDirectory trusted) throws IOException {
        Path current = trusted.path().getRoot();
        int index = 0;
        if (trusted.ancestors().isEmpty()
                || !sameDirectory(trusted.ancestors().get(index++), inspectDirectory(current, false))) {
            throw new IOException("durable store ancestor identity changed: " + current);
        }
        for (Path component : current.relativize(trusted.path())) {
            current = current.resolve(component);
            if (index >= trusted.ancestors().size()
                    || !sameDirectory(trusted.ancestors().get(index++),
                        inspectDirectory(current, current.equals(trusted.path())))) {
                throw new IOException("durable store ancestor identity changed: " + current);
            }
        }
        if (index != trusted.ancestors().size()) {
            throw new IOException("durable store ancestor chain changed: " + trusted.path());
        }
    }

    private static boolean sameDirectory(DirectoryIdentity left, DirectoryIdentity right) {
        return left.path().equals(right.path()) && left.fileKey().equals(right.fileKey())
            && left.owner().equals(right.owner()) && left.permissions().equals(right.permissions());
    }

    private static TrustedFile optionalTrustedFile(Path path) throws IOException {
        try { return trustedFile(path); }
        catch (NoSuchFileException missing) { return null; }
    }

    private static TrustedFile trustedFile(Path path) throws IOException {
        BasicFileAttributes attributes = Files.readAttributes(
            path, BasicFileAttributes.class, LinkOption.NOFOLLOW_LINKS);
        if (!attributes.isRegularFile() || Files.isSymbolicLink(path)
                || attributes.fileKey() == null) {
            throw new IOException("durable store record is not a stable regular file: " + path);
        }
        String pathOwner = owner(path);
        if (!ownerIsCurrent(pathOwner)) {
            throw new IOException("durable store record has an unsafe owner: " + path);
        }
        Set<PosixFilePermission> modes = Files.getPosixFilePermissions(
            path, LinkOption.NOFOLLOW_LINKS);
        if (modes.contains(PosixFilePermission.GROUP_READ)
                || modes.contains(PosixFilePermission.GROUP_WRITE)
                || modes.contains(PosixFilePermission.GROUP_EXECUTE)
                || modes.contains(PosixFilePermission.OTHERS_READ)
                || modes.contains(PosixFilePermission.OTHERS_WRITE)
                || modes.contains(PosixFilePermission.OTHERS_EXECUTE)) {
            throw new IOException("durable store record is accessible beyond its owner: " + path);
        }
        return new TrustedFile(attributes.fileKey(), pathOwner, Set.copyOf(modes), attributes.size());
    }

    private static boolean sameFile(TrustedFile left, TrustedFile right) {
        return left.fileKey().equals(right.fileKey()) && left.owner().equals(right.owner())
            && left.permissions().equals(right.permissions()) && left.size() == right.size();
    }

    private static boolean sameOptionalFile(TrustedFile left, TrustedFile right) {
        return left == null ? right == null : right != null && sameFile(left, right);
    }

    private static String owner(Path path) throws IOException {
        return Files.getOwner(path, LinkOption.NOFOLLOW_LINKS).getName();
    }

    private static boolean ownerAllowed(String owner) {
        return owner.equals("root") || ownerIsCurrent(owner);
    }

    private static boolean ownerIsCurrent(String owner) {
        return owner.equals(CURRENT_OWNER);
    }

    private static String currentOwner() {
        Path probe = null;
        try {
            probe = Files.createTempFile("layerx-owner-", ".probe",
                PosixFilePermissions.asFileAttribute(FILE_PERMISSIONS));
            String owner = Files.getOwner(probe, LinkOption.NOFOLLOW_LINKS).getName();
            if (owner.isEmpty()) throw new IOException("current owner is empty");
            return owner;
        } catch (IOException error) {
            throw new ExceptionInInitializerError(error);
        } finally {
            if (probe != null) {
                try { Files.deleteIfExists(probe); }
                catch (IOException error) { throw new ExceptionInInitializerError(error); }
            }
        }
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
