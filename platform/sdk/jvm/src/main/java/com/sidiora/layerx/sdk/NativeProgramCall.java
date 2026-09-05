package com.sidiora.layerx.sdk;

import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;
import java.util.Arrays;

public record NativeProgramCall(byte[] programId, int guestAbi, String entrypoint, byte[] calldata,
        byte[] capabilities, byte[] accessDeclaration, int responseCapacity, long[] resources) {
    public byte[] encode() {
        if (programId.length != 32 || Arrays.equals(programId, new byte[32]) || (guestAbi != 1 && guestAbi != 2)
                || !entrypoint.matches("[A-Za-z0-9_.]{1,128}") || calldata.length > 1048576
                || capabilities.length > 65535 || accessDeclaration.length > 1048576
                || responseCapacity < 0 || responseCapacity > 1048576 || resources.length != 7)
            throw new IllegalArgumentException("invalid native program call");
        byte[] entry = entrypoint.getBytes(StandardCharsets.US_ASCII);
        ByteBuffer out = ByteBuffer.allocate(106 + entry.length + calldata.length + capabilities.length + accessDeclaration.length);
        out.put(programId).putShort((short)guestAbi).putShort((short)entry.length).putInt(calldata.length)
            .putShort((short)capabilities.length).putInt(accessDeclaration.length).putInt(responseCapacity);
        for (long resource : resources) out.putLong(resource);
        return out.put(entry).put(calldata).put(capabilities).put(accessDeclaration).array();
    }

    public static NativeProgramCall decode(byte[] payload) {
        if (payload.length < 106) throw new IllegalArgumentException("invalid native program call");
        ByteBuffer input = ByteBuffer.wrap(payload);
        byte[] program = new byte[32]; input.get(program);
        int abi = Short.toUnsignedInt(input.getShort());
        int entry = Short.toUnsignedInt(input.getShort()); long data = Integer.toUnsignedLong(input.getInt());
        int caps = Short.toUnsignedInt(input.getShort()); long access = Integer.toUnsignedLong(input.getInt());
        int capacity = input.getInt(); long[] resources = new long[7];
        for (int i = 0; i < 7; i++) resources[i] = input.getLong();
        if (106L + entry + data + caps + access != payload.length) throw new IllegalArgumentException("invalid native program call");
        byte[] entryBytes = new byte[entry], dataBytes = new byte[(int)data], capBytes = new byte[caps], accessBytes = new byte[(int)access];
        input.get(entryBytes).get(dataBytes).get(capBytes).get(accessBytes);
        NativeProgramCall call = new NativeProgramCall(program, abi, new String(entryBytes, StandardCharsets.US_ASCII), dataBytes, capBytes, accessBytes, capacity, resources);
        if (!Arrays.equals(call.encode(), payload)) throw new IllegalArgumentException("invalid native program call");
        return call;
    }
}
