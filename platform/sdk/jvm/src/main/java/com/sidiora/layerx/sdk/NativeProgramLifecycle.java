package com.sidiora.layerx.sdk;

import java.nio.ByteBuffer;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.Arrays;

public sealed interface NativeProgramLifecycle permits NativeProgramDeploy, NativeProgramUpgrade, NativeProgramWindDown {
    int ordinal();
    byte[] encode();

    static NativeProgramLifecycle decode(int ordinal, byte[] payload) {
        return switch (ordinal) {
            case 1 -> NativeProgramDeploy.decode(payload);
            case 2 -> NativeProgramUpgrade.decode(payload);
            case 7 -> NativeProgramWindDown.decode(payload);
            default -> throw new IllegalArgumentException("Programs lifecycle ordinal");
        };
    }

    static byte[] hash(byte[]... parts) {
        try {
            MessageDigest digest = MessageDigest.getInstance("SHA-256");
            for (byte[] part : parts) digest.update(part);
            return digest.digest();
        } catch (NoSuchAlgorithmException error) { throw new IllegalStateException(error); }
    }

    static void require(boolean condition) {
        if (!condition) throw new IllegalArgumentException("Non-canonical Programs lifecycle");
    }

    static void bytes32(byte[] value) { require(value != null && value.length == 32); }

    static void code(byte[] program, int abi, byte[] hash, byte[] wasm) {
        bytes32(program); bytes32(hash);
        require(!Arrays.equals(program, new byte[32]) && abi >= 1 && abi <= 3
            && wasm != null && wasm.length >= 8 && wasm.length <= 1048576
            && Arrays.equals(Arrays.copyOf(wasm, 8), new byte[]{0, 97, 115, 109, 1, 0, 0, 0})
            && MessageDigest.isEqual(hash, hash(wasm)));
    }

    static long u32(byte[] payload, int offset) { return Integer.toUnsignedLong(ByteBuffer.wrap(payload).getInt(offset)); }
    static int u16(byte[] payload, int offset) { return Short.toUnsignedInt(ByteBuffer.wrap(payload).getShort(offset)); }
}
