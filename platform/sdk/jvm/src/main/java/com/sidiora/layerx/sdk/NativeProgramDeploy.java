package com.sidiora.layerx.sdk;

import java.nio.ByteBuffer;
import java.util.Arrays;
import static com.sidiora.layerx.sdk.NativeProgramLifecycle.*;

public record NativeProgramDeploy(byte[] programId, int guestAbi, int policy, byte[] authority,
                                  byte[] newHash, byte[] programInterface, byte[] wasm) implements NativeProgramLifecycle {
    public NativeProgramDeploy {
        code(programId, guestAbi, newHash, wasm); bytes32(authority);
        require(policy >= 0 && policy <= 1 && (policy == 0) == Arrays.equals(authority, new byte[32]));
        require(programInterface == null || programInterface.length > 0 && programInterface.length <= 952);
        programId = programId.clone(); authority = authority.clone(); newHash = newHash.clone(); wasm = wasm.clone();
        programInterface = programInterface == null ? null : programInterface.clone();
    }
    @Override public byte[] programId() { return programId.clone(); }
    @Override public byte[] authority() { return authority.clone(); }
    @Override public byte[] newHash() { return newHash.clone(); }
    @Override public byte[] programInterface() { return programInterface == null ? null : programInterface.clone(); }
    @Override public byte[] wasm() { return wasm.clone(); }
    @Override public int ordinal() { return 1; }
    @Override public byte[] encode() {
        ByteBuffer output = ByteBuffer.allocate(104 + (programInterface == null ? 0 : 4 + programInterface.length) + wasm.length);
        output.put(programId).putShort((short) guestAbi).put((byte) policy).put((byte) 0).put(authority).put(newHash).putInt(wasm.length);
        if (programInterface != null) output.putInt(programInterface.length).put(programInterface);
        return output.put(wasm).array();
    }
    public static NativeProgramDeploy decode(byte[] payload) {
        require(payload != null && payload.length >= 104 && payload[35] == 0);
        long wasmLength = u32(payload, 100); int offset = 104; byte[] programInterface = null;
        if (wasmLength != payload.length - 104L) {
            require(payload.length >= 108); long length = u32(payload, 104);
            require(length > 0 && length <= 952 && 108 + length + wasmLength == payload.length);
            offset = 108 + (int) length; programInterface = Arrays.copyOfRange(payload, 108, offset);
        }
        NativeProgramDeploy value = new NativeProgramDeploy(Arrays.copyOf(payload, 32), u16(payload, 32), Byte.toUnsignedInt(payload[34]),
            Arrays.copyOfRange(payload, 36, 68), Arrays.copyOfRange(payload, 68, 100), programInterface, Arrays.copyOfRange(payload, offset, payload.length));
        require(Arrays.equals(value.encode(), payload)); return value;
    }
}
