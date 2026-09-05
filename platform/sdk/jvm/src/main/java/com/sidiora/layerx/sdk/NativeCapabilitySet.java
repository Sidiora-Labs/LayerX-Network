package com.sidiora.layerx.sdk;

import java.io.ByteArrayOutputStream;
import java.math.BigInteger;
import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;

public final class NativeCapabilitySet {
    public static final int MAXIMUM_GRANTS = 238;
    public static final int MAXIMUM_BYTES = 65452;
    public static final int MAXIMUM_BALANCE_VIEWS = 32;

    public sealed interface Capability permits StorageRead, StorageWrite, EmitEvent, Call,
            Transfer402, ProgramSpend, ReceiptRead, BalanceView, SharedStorageRead, SharedStorageWrite {}
    public record StorageRead() implements Capability {}
    public record StorageWrite() implements Capability {}
    public record EmitEvent() implements Capability {}
    public record Call(byte[] program) implements Capability {}
    public record Transfer402(byte[] asset, byte[] to, BigInteger maximumAmount) implements Capability {}
    public record ProgramSpend(byte[] ownerProgram, byte[] seed, byte[] sourceAccount, byte[] asset,
                               byte[] to, BigInteger maximumAmount) implements Capability {}
    public record ReceiptRead(byte[] receiptDigest) implements Capability {}
    public record BalanceView(byte[] account, byte[] asset, byte[] receiptDigest) implements Capability {}
    public record SharedStorageRead() implements Capability {}
    public record SharedStorageWrite() implements Capability {}

    private record Entry(int rank, List<byte[]> key, byte[] encoded, BigInteger maximum, byte[] receipt) {}
    private NativeCapabilitySet() {}
    private static void require(boolean condition) {
        if (!condition) throw new IllegalArgumentException("Noncanonical native capability set");
    }
    private static byte[] nonzero(byte[] value) {
        require(value != null && value.length == 32 && !Arrays.equals(value, new byte[32]));
        return value.clone();
    }
    public static byte[] deriveProgramAccount(byte[] owner, byte[] seed) {
        byte[] program = nonzero(owner);
        require(seed != null && seed.length <= 128);
        try {
            MessageDigest hash = MessageDigest.getInstance("SHA-256");
            hash.update("LayerX/programs/program-account/v1\0".getBytes(StandardCharsets.UTF_8));
            hash.update(program); hash.update(ByteBuffer.allocate(4).putInt(seed.length).array());
            return hash.digest(seed);
        } catch (NoSuchAlgorithmException error) { throw new IllegalStateException(error); }
    }
    private static byte[] amount(BigInteger value) {
        require(value != null && value.signum() > 0 && value.bitLength() <= 128);
        byte[] source = value.toByteArray(); byte[] result = new byte[16];
        int length = Math.min(source.length, 16);
        System.arraycopy(source, source.length - length, result, 16 - length, length);
        return result;
    }
    private static Entry entry(Capability grant) {
        require(grant != null);
        var output = new ByteArrayOutputStream(); var key = new ArrayList<byte[]>();
        int rank; BigInteger maximum = BigInteger.ZERO; byte[] receipt = new byte[0];
        if (grant instanceof StorageRead) { rank = 0; output.write(1); }
        else if (grant instanceof StorageWrite) { rank = 1; output.write(2); }
        else if (grant instanceof EmitEvent) { rank = 2; output.write(3); }
        else if (grant instanceof Call value) { rank = 3; output.write(4); key.add(nonzero(value.program())); }
        else if (grant instanceof Transfer402 value) {
            rank = 4; output.write(5); key.add(nonzero(value.asset())); key.add(nonzero(value.to())); maximum = value.maximumAmount();
        } else if (grant instanceof ProgramSpend value) {
            rank = 5; output.write(9);
            byte[] owner = nonzero(value.ownerProgram());
            require(value.seed() != null && value.seed().length <= 128);
            byte[] seed = value.seed().clone();
            require(Arrays.equals(deriveProgramAccount(owner, seed), value.sourceAccount()));
            key.add(owner); key.add(seed); key.add(value.sourceAccount().clone());
            key.add(nonzero(value.asset())); key.add(nonzero(value.to())); maximum = value.maximumAmount();
        } else if (grant instanceof ReceiptRead value) { rank = 6; output.write(6); key.add(nonzero(value.receiptDigest())); }
        else if (grant instanceof BalanceView value) {
            rank = 7; output.write(10); key.add(nonzero(value.account())); key.add(nonzero(value.asset())); receipt = nonzero(value.receiptDigest());
        } else if (grant instanceof SharedStorageRead) { rank = 8; output.write(7); }
        else if (grant instanceof SharedStorageWrite) { rank = 9; output.write(8); }
        else throw new IllegalArgumentException("Unknown native capability");
        for (int index = 0; index < key.size(); index++) {
            byte[] field = key.get(index);
            if (rank == 5 && index == 1) { output.write(field.length >>> 8); output.write(field.length); }
            output.writeBytes(field);
        }
        if (rank == 4 || rank == 5) output.writeBytes(amount(maximum));
        output.writeBytes(receipt);
        return new Entry(rank, key, output.toByteArray(), maximum, receipt);
    }
    private static int compare(Entry left, Entry right) {
        int rank = Integer.compare(left.rank(), right.rank());
        if (rank != 0) return rank;
        for (int index = 0; index < left.key().size(); index++) {
            int order = Arrays.compareUnsigned(left.key().get(index), right.key().get(index));
            if (order != 0) return order;
        }
        return 0;
    }
    public static byte[] encode(List<? extends Capability> grants) {
        require(grants != null && grants.size() <= MAXIMUM_GRANTS);
        var entries = new ArrayList<Entry>(); int balanceViews = 0;
        for (Capability grant : grants) {
            Entry value = entry(grant); entries.add(value);
            if (value.rank() == 7) balanceViews++;
        }
        require(balanceViews <= MAXIMUM_BALANCE_VIEWS);
        entries.sort(NativeCapabilitySet::compare);
        var output = new ByteArrayOutputStream(); output.write(entries.size() >>> 8); output.write(entries.size());
        Entry prior = null;
        for (Entry value : entries) {
            require(prior == null || compare(prior, value) < 0);
            output.writeBytes(value.encoded()); prior = value;
        }
        require(output.size() <= MAXIMUM_BYTES);
        return output.toByteArray();
    }
    private static byte[] take(ByteBuffer cursor, int length) {
        require(length >= 0 && length <= cursor.remaining());
        byte[] value = new byte[length]; cursor.get(value); return value;
    }
    private static int word(ByteBuffer cursor) {
        return Short.toUnsignedInt(ByteBuffer.wrap(take(cursor, 2)).getShort());
    }
    private static BigInteger amount(ByteBuffer cursor) { return new BigInteger(1, take(cursor, 16)); }
    public static List<Capability> decode(byte[] encoded) {
        require(encoded != null && encoded.length >= 2 && encoded.length <= MAXIMUM_BYTES);
        ByteBuffer cursor = ByteBuffer.wrap(encoded); int count = word(cursor);
        require(count <= MAXIMUM_GRANTS); var grants = new ArrayList<Capability>();
        for (int index = 0; index < count; index++) {
            int tag = Byte.toUnsignedInt(take(cursor, 1)[0]);
            Capability grant;
            switch (tag) {
                case 1 -> grant = new StorageRead();
                case 2 -> grant = new StorageWrite();
                case 3 -> grant = new EmitEvent();
                case 4 -> grant = new Call(take(cursor, 32));
                case 5 -> grant = new Transfer402(take(cursor, 32), take(cursor, 32), amount(cursor));
                case 6 -> grant = new ReceiptRead(take(cursor, 32));
                case 7 -> grant = new SharedStorageRead();
                case 8 -> grant = new SharedStorageWrite();
                case 9 -> {
                    byte[] owner = take(cursor, 32); int length = word(cursor); require(length <= 128);
                    grant = new ProgramSpend(owner, take(cursor, length), take(cursor, 32), take(cursor, 32), take(cursor, 32), amount(cursor));
                }
                case 10 -> grant = new BalanceView(take(cursor, 32), take(cursor, 32), take(cursor, 32));
                default -> throw new IllegalArgumentException("Unknown native capability tag");
            }
            grants.add(grant);
        }
        require(!cursor.hasRemaining() && Arrays.equals(encode(grants), encoded));
        return List.copyOf(grants);
    }
    public static List<Capability> narrow(List<? extends Capability> parent, List<? extends Capability> requested) {
        List<Capability> parents = decode(encode(parent)); List<Capability> children = decode(encode(requested));
        for (Capability child : children) {
            Entry value = entry(child); Entry found = null;
            for (Capability grant : parents) {
                Entry ancestor = entry(grant);
                if (compare(ancestor, value) == 0) { found = ancestor; break; }
            }
            require(found != null && value.maximum().compareTo(found.maximum()) <= 0 && Arrays.equals(value.receipt(), found.receipt()));
        }
        return children;
    }
}
