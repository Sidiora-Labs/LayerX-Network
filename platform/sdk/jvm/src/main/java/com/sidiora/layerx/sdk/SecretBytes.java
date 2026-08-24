package com.sidiora.layerx.sdk;

import com.fasterxml.jackson.annotation.JsonIgnoreType;
import java.util.Arrays;
import java.util.Objects;
import java.util.function.Function;

/** Owned secret bytes with explicit zeroization and redacted diagnostics. */
@JsonIgnoreType
public final class SecretBytes implements AutoCloseable {
    private final byte[] bytes;
    private boolean destroyed;

    public SecretBytes(byte[] bytes) {
        Objects.requireNonNull(bytes, "bytes");
        if (bytes.length == 0) throw PlatformSdkException.invalidArgument();
        this.bytes = bytes.clone();
    }

    public synchronized <T> T use(Function<byte[], T> consumer) {
        Objects.requireNonNull(consumer, "consumer");
        if (destroyed) throw PlatformSdkException.invalidArgument();
        byte[] temporary = bytes.clone();
        try {
            return consumer.apply(temporary);
        } finally {
            Arrays.fill(temporary, (byte) 0);
        }
    }

    public synchronized boolean isDestroyed() {
        return destroyed;
    }

    @Override
    public synchronized void close() {
        Arrays.fill(bytes, (byte) 0);
        destroyed = true;
    }

    @Override public String toString() { return "[REDACTED]"; }
}
