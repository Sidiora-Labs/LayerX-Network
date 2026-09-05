package com.sidiora.layerx.sdk;

import java.nio.ByteBuffer;
import java.util.Arrays;
import static com.sidiora.layerx.sdk.NativeProgramLifecycle.*;

public final class NativeProgramWindDown implements NativeProgramLifecycle {
    private final byte[] payload;
    private NativeProgramWindDown(byte[] payload) { this.payload = payload.clone(); }
    @Override public int ordinal() { return 7; }
    @Override public byte[] encode() { return payload.clone(); }
    public int operation() { return payload[32]; }
    public byte[] programId() { return Arrays.copyOf(payload, 32); }
    public static NativeProgramWindDown route(byte[] program, byte[] account, byte[] asset, byte[] destination, byte[] seed) {
        bytes32(program); bytes32(account); bytes32(asset); bytes32(destination); require(seed != null && seed.length <= 128);
        return decode(ByteBuffer.allocate(131 + seed.length).put(program).put((byte) 1).put(account).put(asset).put(destination).putShort((short) seed.length).put(seed).array());
    }
    public static NativeProgramWindDown deprecate(byte[] program, byte[] exitProgram, long deadlineBatch) {
        bytes32(program); bytes32(exitProgram);
        return decode(ByteBuffer.allocate(73).put(program).put((byte) 2).put(exitProgram).putLong(deadlineBatch).array());
    }
    public static NativeProgramWindDown tombstone(byte[] program) {
        bytes32(program); return decode(ByteBuffer.allocate(33).put(program).put((byte) 3).array());
    }
    public static NativeProgramWindDown exit(byte[] program, byte[] account) {
        bytes32(program); bytes32(account); return decode(ByteBuffer.allocate(65).put(program).put((byte) 4).put(account).array());
    }
    public static NativeProgramWindDown decode(byte[] payload) {
        require(payload != null && payload.length >= 33 && !Arrays.equals(Arrays.copyOf(payload, 32), new byte[32]));
        switch (payload[32]) {
            case 1 -> { require(payload.length >= 131); int length = u16(payload, 129); require(length <= 128 && payload.length == 131 + length); }
            case 2 -> require(payload.length == 73);
            case 3 -> require(payload.length == 33);
            case 4 -> require(payload.length == 65);
            default -> throw new IllegalArgumentException("Wind-down operation");
        }
        return new NativeProgramWindDown(payload);
    }
}
