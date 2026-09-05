package com.sidiora.layerx.sdk;

import java.nio.ByteBuffer;
import java.util.Arrays;
import static com.sidiora.layerx.sdk.NativeProgramLifecycle.*;

public record NativeProgramUpgrade(byte[] programId, int guestAbi, byte[] oldHash, byte[] newHash,
                                   byte[] migrationHook, boolean clearInterface, byte[] programInterface,
                                   byte[] wasm) implements NativeProgramLifecycle {
    public NativeProgramUpgrade {
        code(programId, guestAbi, newHash, wasm); bytes32(oldHash);
        require(migrationHook != null && migrationHook.length <= 65535 && (!clearInterface || programInterface != null)
            && (programInterface == null || programInterface.length <= 952 && (programInterface.length > 0 || clearInterface)));
        programId = programId.clone(); oldHash = oldHash.clone(); newHash = newHash.clone(); migrationHook = migrationHook.clone(); wasm = wasm.clone();
        programInterface = programInterface == null ? null : programInterface.clone();
    }
    @Override public byte[] programId() { return programId.clone(); }
    @Override public byte[] oldHash() { return oldHash.clone(); }
    @Override public byte[] newHash() { return newHash.clone(); }
    @Override public byte[] migrationHook() { return migrationHook.clone(); }
    @Override public byte[] programInterface() { return programInterface == null ? null : programInterface.clone(); }
    @Override public byte[] wasm() { return wasm.clone(); }
    @Override public int ordinal() { return 2; }
    @Override public byte[] encode() {
        ByteBuffer output = ByteBuffer.allocate(106 + (programInterface == null ? 0 : 4 + programInterface.length) + migrationHook.length + wasm.length);
        output.put(programId).putShort((short) guestAbi).put((byte) ((migrationHook.length > 0 ? 1 : 0) | (clearInterface ? 2 : 0))).put((byte) 0)
            .put(oldHash).put(newHash).putShort((short) migrationHook.length).putInt(wasm.length);
        if (programInterface != null) output.putInt(programInterface.length);
        output.put(migrationHook); if (programInterface != null) output.put(programInterface);
        return output.put(wasm).array();
    }
    public static NativeProgramUpgrade decode(byte[] payload) {
        require(payload != null && payload.length >= 106 && payload[35] == 0 && (payload[34] & 0xfc) == 0);
        int hook = u16(payload, 100); long wasmLength = u32(payload, 102); boolean clear = (payload[34] & 2) != 0;
        require(((payload[34] & 1) == 0) == (hook == 0)); int offset = 106; int length = 0; byte[] programInterface = null;
        if (clear || hook + wasmLength != payload.length - 106L) {
            require(payload.length >= 110); long size = u32(payload, 106);
            require(size <= 952 && (size > 0 || clear) && 110 + hook + size + wasmLength == payload.length);
            offset = 110; length = (int) size; programInterface = Arrays.copyOfRange(payload, offset + hook, offset + hook + length);
        }
        NativeProgramUpgrade value = new NativeProgramUpgrade(Arrays.copyOf(payload, 32), u16(payload, 32), Arrays.copyOfRange(payload, 36, 68),
            Arrays.copyOfRange(payload, 68, 100), Arrays.copyOfRange(payload, offset, offset + hook), clear, programInterface,
            Arrays.copyOfRange(payload, offset + hook + length, payload.length));
        require(Arrays.equals(value.encode(), payload)); return value;
    }
}
