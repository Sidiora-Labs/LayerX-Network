package com.sidiora.layerx.sdk;

import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;
import java.util.Arrays;
import java.util.HexFormat;
import static com.sidiora.layerx.sdk.NativeProgramLifecycle.*;

public final class NativeProgramLifecycleRequest {
    private final NativeProgramLifecycle operation;
    private final byte[] signed;
    private final byte[] activityId;
    private final byte[] idempotencyKey;

    public NativeProgramLifecycleRequest(NativeProgramLifecycle operation, byte[] signedActivity) {
        require(operation != null);
        this.operation = NativeProgramLifecycle.decode(operation.ordinal(), operation.encode());
        this.signed = signedActivity.clone();
        idempotencyKey = bind(operation.ordinal(), operation.encode(), signed);
        activityId = hash("LXP/v1/activity-id\0".getBytes(StandardCharsets.UTF_8), signed);
    }
    public int ordinal() { return operation.ordinal(); }
    public byte[] payload() { return operation.encode(); }
    public byte[] signedActivity() { return signed.clone(); }
    public byte[] activityId() { return activityId.clone(); }
    public String idempotencyKey() { return HexFormat.of().formatHex(idempotencyKey); }

    static byte[] bind(int ordinal, byte[] expected, byte[] signed) {
        require(signed != null && signed.length > 0 && signed.length <= 1048576);
        try {
            ByteBuffer cursor = ByteBuffer.wrap(signed);
            require(cursor.getShort() == 3 && cursor.getShort() == 0x1001 && cursor.get() == 12 && cursor.get() == 1 && cursor.getShort() == 3 && cursor.get() == 2);
            cursor.getInt(); require(cursor.get() == 3 && cursor.getInt() == (0x00090000 | ordinal) && cursor.get() == 4);
            bounded(cursor, 255); require(cursor.get() == 5); bounded(cursor, 524288);
            require(cursor.get() == 6); cursor.getLong(); require(cursor.get() == 7);
            long before = cursor.getLong(); long after = cursor.getLong(); require(Long.compareUnsigned(after, before) >= 0 && cursor.get() == 8);
            byte[] key = bounded(cursor, 32); require(key.length == 32 && cursor.get() == 9);
            cursor.getLong(); cursor.getLong(); require(cursor.get() == 10); byte[] digest = bounded(cursor, 32);
            require(digest.length == 32 && cursor.get() == 11); byte[] payload = bounded(cursor, 524288);
            require(cursor.get() == 12); bounded(cursor, 128);
            require(!cursor.hasRemaining() && Arrays.equals(payload, expected) && Arrays.equals(digest, hash("LXP/v1/payload-hash\0".getBytes(StandardCharsets.UTF_8), payload)));
            return key;
        } catch (java.nio.BufferUnderflowException error) { throw new IllegalArgumentException("Truncated signed activity", error); }
    }

    static byte[] bounded(ByteBuffer cursor, int maximum) {
        long length = Integer.toUnsignedLong(cursor.getInt()); require(length <= maximum && length <= cursor.remaining());
        byte[] value = new byte[(int) length]; cursor.get(value); return value;
    }
}
