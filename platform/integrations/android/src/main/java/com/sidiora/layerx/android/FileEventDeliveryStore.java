package com.sidiora.layerx.android;

import com.fasterxml.jackson.core.type.TypeReference;
import com.fasterxml.jackson.databind.ObjectMapper;
import java.io.IOException;
import java.nio.ByteBuffer;
import java.nio.channels.FileChannel;
import java.nio.channels.FileLock;
import java.nio.charset.StandardCharsets;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.nio.file.StandardOpenOption;
import java.nio.file.attribute.PosixFilePermissions;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Objects;
import java.util.function.LongSupplier;

/** Process-safe, crash-durable delivery claims stored in the application's private filesystem. */
public final class FileEventDeliveryStore implements EventDeliveryStore {
    private static final long MAXIMUM_LEDGER_BYTES = 32L * 1024L * 1024L;

    private record Entry(String payloadDigest, long leaseUntilMs, boolean completed) {}
    private record Ledger(int version, Map<String, Entry> entries) {}

    @FunctionalInterface
    private interface Mutation<T> { T apply(Map<String, Entry> entries); }

    private final Path ledgerPath;
    private final Path lockPath;
    private final ObjectMapper mapper;
    private final int capacity;
    private final LongSupplier clock;

    public FileEventDeliveryStore(Path ledgerPath) {
        this(ledgerPath, 65_536, null);
    }

    public FileEventDeliveryStore(Path ledgerPath, int capacity, LongSupplier clock) {
        this.ledgerPath = Objects.requireNonNull(ledgerPath, "ledgerPath").toAbsolutePath().normalize();
        if (this.ledgerPath.getParent() == null || capacity < 1) {
            throw MobileIntegrationException.of(MobileIntegrationException.Code.INVALID_CONFIGURATION);
        }
        this.lockPath = this.ledgerPath.resolveSibling(this.ledgerPath.getFileName() + ".lock");
        this.mapper = new ObjectMapper();
        this.capacity = capacity;
        this.clock = clock == null ? System::currentTimeMillis : clock;
    }

    public static Path defaultPath() {
        String configured = System.getProperty("com.sidiora.layerx.deliveryStore");
        if (configured != null && !configured.isBlank()) return Path.of(configured);
        String home = System.getProperty("user.home");
        if (home == null || home.isBlank()) {
            throw MobileIntegrationException.of(MobileIntegrationException.Code.INVALID_CONFIGURATION);
        }
        return Path.of(home, ".layerx", "android-event-deliveries-v1.json");
    }

    @Override
    public Claim claim(String deliveryId, String payloadDigest, long leaseUntilMs) {
        requireClaim(deliveryId, payloadDigest, leaseUntilMs);
        return mutate(entries -> {
            Entry existing = entries.get(deliveryId);
            if (existing != null) {
                if (!existing.payloadDigest().equals(payloadDigest)) return Claim.CONFLICT;
                if (existing.completed()) return Claim.COMPLETED;
                if (existing.leaseUntilMs() > clock.getAsLong()) return Claim.PROCESSING;
                entries.put(deliveryId, new Entry(payloadDigest, leaseUntilMs, false));
                return Claim.CLAIMED;
            }
            if (entries.size() >= capacity) evict(entries);
            if (entries.size() >= capacity) throw failure();
            entries.put(deliveryId, new Entry(payloadDigest, leaseUntilMs, false));
            return Claim.CLAIMED;
        });
    }

    @Override
    public void complete(String deliveryId, String payloadDigest) {
        requireIdentifier(deliveryId);
        requireDigest(payloadDigest);
        mutate(entries -> {
            Entry existing = entries.get(deliveryId);
            if (existing == null || !existing.payloadDigest().equals(payloadDigest)) {
                throw MobileIntegrationException.of(MobileIntegrationException.Code.EVENT_REPLAY);
            }
            entries.put(deliveryId, new Entry(payloadDigest, 0L, true));
            return null;
        });
    }

    @Override
    public void release(String deliveryId, String payloadDigest) {
        requireIdentifier(deliveryId);
        requireDigest(payloadDigest);
        mutate(entries -> {
            Entry existing = entries.get(deliveryId);
            if (existing != null && existing.payloadDigest().equals(payloadDigest) && !existing.completed()) {
                entries.remove(deliveryId);
            }
            return null;
        });
    }

    private <T> T mutate(Mutation<T> mutation) {
        Path directory = ledgerPath.getParent();
        try {
            Files.createDirectories(directory);
            setDirectoryPermissions(directory);
            try (FileChannel channel = FileChannel.open(lockPath,
                     StandardOpenOption.CREATE, StandardOpenOption.WRITE);
                 FileLock ignored = channel.lock()) {
                Map<String, Entry> entries = read();
                T result = mutation.apply(entries);
                write(entries);
                return result;
            }
        } catch (MobileIntegrationException error) {
            throw error;
        } catch (IOException | RuntimeException error) {
            throw failure();
        }
    }

    private Map<String, Entry> read() throws IOException {
        if (!Files.exists(ledgerPath)) return new LinkedHashMap<>();
        if (!Files.isRegularFile(ledgerPath) || Files.size(ledgerPath) > MAXIMUM_LEDGER_BYTES) throw failure();
        Ledger ledger = mapper.readValue(Files.readAllBytes(ledgerPath), new TypeReference<Ledger>() {});
        if (ledger == null || ledger.version() != 1 || ledger.entries() == null) throw failure();
        Map<String, Entry> entries = new LinkedHashMap<>();
        for (Map.Entry<String, Entry> item : ledger.entries().entrySet()) {
            Entry entry = item.getValue();
            requireIdentifier(item.getKey());
            if (entry == null) throw failure();
            requireDigest(entry.payloadDigest());
            if (entry.leaseUntilMs() < 0L) throw failure();
            entries.put(item.getKey(), entry);
        }
        return entries;
    }

    private void write(Map<String, Entry> entries) throws IOException {
        byte[] encoded = mapper.writeValueAsBytes(new Ledger(1, entries));
        if (encoded.length > MAXIMUM_LEDGER_BYTES) throw failure();
        Path temporary = Files.createTempFile(ledgerPath.getParent(), ".layerx-deliveries-", ".tmp");
        try {
            setFilePermissions(temporary);
            try (FileChannel output = FileChannel.open(temporary, StandardOpenOption.WRITE,
                    StandardOpenOption.TRUNCATE_EXISTING)) {
                ByteBuffer bytes = ByteBuffer.wrap(encoded);
                while (bytes.hasRemaining()) output.write(bytes);
                output.force(true);
            }
            try {
                Files.move(temporary, ledgerPath, StandardCopyOption.ATOMIC_MOVE,
                    StandardCopyOption.REPLACE_EXISTING);
            } catch (AtomicMoveNotSupportedException error) {
                throw failure();
            }
            setFilePermissions(ledgerPath);
        } finally {
            Files.deleteIfExists(temporary);
        }
    }

    private void evict(Map<String, Entry> entries) {
        long now = clock.getAsLong();
        entries.entrySet().removeIf(item -> !item.getValue().completed() && item.getValue().leaseUntilMs() <= now);
    }

    private static void setDirectoryPermissions(Path directory) throws IOException {
        Files.setPosixFilePermissions(directory, PosixFilePermissions.fromString("rwx------"));
    }

    private static void setFilePermissions(Path file) throws IOException {
        Files.setPosixFilePermissions(file, PosixFilePermissions.fromString("rw-------"));
    }

    private static void requireClaim(String deliveryId, String payloadDigest, long leaseUntilMs) {
        requireIdentifier(deliveryId);
        requireDigest(payloadDigest);
        if (leaseUntilMs <= 0L) throw failure();
    }

    private static void requireIdentifier(String value) {
        if (value == null || value.isEmpty() || value.getBytes(StandardCharsets.UTF_8).length > 255
                || value.indexOf('\0') >= 0) throw failure();
    }

    private static void requireDigest(String value) {
        if (value == null || !value.matches("[0-9a-f]{64}")) throw failure();
    }

    private static MobileIntegrationException failure() {
        return MobileIntegrationException.of(MobileIntegrationException.Code.DELIVERY_STORE_FAILURE);
    }
}
