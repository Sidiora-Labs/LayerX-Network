package com.sidiora.layerx.sdk;

/** Generated from platform/sdk/schema/mirror-v2.kvx. */
public final class GeneratedMirror {
    public static final int SCHEMA_VERSION = 2;
    public static final int MAX_ARCHIVE_BYTES = 67_108_864;
    public static final int MAX_SOURCES = 8;
    public static final int MAX_JSON_DEPTH = 32;
    public enum Policy { EXACT, ORDERED_PREFERENCE, AGREEMENT }
    public enum Error { CONFIGURATION, UNAVAILABLE, RATE_LIMITED, MISSING,
        TARGET_MISMATCH, SOURCE_MISMATCH, MALFORMED, BOUNDS, COMMITMENT,
        AUTHORIZATION, PROOF, CHECKPOINT_UNAVAILABLE, DIVERGENT,
        INSUFFICIENT_AGREEMENT, REORGED }
    private GeneratedMirror() {}
}
