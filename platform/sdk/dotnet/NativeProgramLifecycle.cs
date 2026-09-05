using System.Buffers.Binary;
using System.Security.Cryptography;
using System.Text;

namespace LayerX.Sdk;

public interface INativeProgramLifecycle
{
    ushort Ordinal { get; }
    byte[] Encode();
}

public sealed class NativeProgramDeploy : INativeProgramLifecycle
{
    private readonly byte[] _payload;
    public ushort Ordinal => 1;
    public NativeProgramDeploy(byte[] programId, ushort guestAbi, byte policy, byte[] authority,
        byte[] newHash, byte[]? programInterface, byte[] wasm)
    {
        LifecycleWire.Code(programId, guestAbi, newHash, wasm); LifecycleWire.Bytes32(authority);
        LifecycleWire.Require(policy <= 1 && (policy == 0) == authority.All(value => value == 0) &&
            (programInterface is null || programInterface.Length is > 0 and <= 952));
        using var output = new MemoryStream();
        output.Write(programId); LifecycleWire.Put(output, guestAbi, 2); output.WriteByte(policy); output.WriteByte(0);
        output.Write(authority); output.Write(newHash); LifecycleWire.Put(output, (ulong)wasm.Length, 4);
        if (programInterface is not null) { LifecycleWire.Put(output, (ulong)programInterface.Length, 4); output.Write(programInterface); }
        output.Write(wasm); _payload = output.ToArray();
    }
    public byte[] Encode() => _payload.ToArray();
    public static NativeProgramDeploy Decode(byte[] payload)
    {
        LifecycleWire.Require(payload.Length >= 104 && payload[35] == 0);
        var wasm = LifecycleWire.Get(payload, 100, 4); var offset = 104; byte[]? programInterface = null;
        if (wasm != (ulong)(payload.Length - 104))
        {
            LifecycleWire.Require(payload.Length >= 108); var length = LifecycleWire.Get(payload, 104, 4);
            LifecycleWire.Require(length is > 0 and <= 952 && 108 + length + wasm == (ulong)payload.Length);
            offset = 108 + (int)length; programInterface = payload[108..offset];
        }
        var value = new NativeProgramDeploy(payload[..32], (ushort)LifecycleWire.Get(payload, 32, 2), payload[34],
            payload[36..68], payload[68..100], programInterface, payload[offset..]);
        LifecycleWire.Require(value.Encode().SequenceEqual(payload)); return value;
    }
}

public sealed class NativeProgramUpgrade : INativeProgramLifecycle
{
    private readonly byte[] _payload;
    public ushort Ordinal => 2;
    public NativeProgramUpgrade(byte[] programId, ushort guestAbi, byte[] oldHash, byte[] newHash,
        byte[] migrationHook, bool clearInterface, byte[]? programInterface, byte[] wasm)
    {
        LifecycleWire.Code(programId, guestAbi, newHash, wasm); LifecycleWire.Bytes32(oldHash);
        LifecycleWire.Require(migrationHook.Length <= ushort.MaxValue && (!clearInterface || programInterface is not null) &&
            (programInterface is null || programInterface.Length <= 952 && (programInterface.Length > 0 || clearInterface)));
        using var output = new MemoryStream();
        output.Write(programId); LifecycleWire.Put(output, guestAbi, 2);
        output.WriteByte((byte)((migrationHook.Length > 0 ? 1 : 0) | (clearInterface ? 2 : 0))); output.WriteByte(0);
        output.Write(oldHash); output.Write(newHash); LifecycleWire.Put(output, (ulong)migrationHook.Length, 2); LifecycleWire.Put(output, (ulong)wasm.Length, 4);
        if (programInterface is not null) LifecycleWire.Put(output, (ulong)programInterface.Length, 4);
        output.Write(migrationHook); if (programInterface is not null) output.Write(programInterface);
        output.Write(wasm); _payload = output.ToArray();
    }
    public byte[] Encode() => _payload.ToArray();
    public static NativeProgramUpgrade Decode(byte[] payload)
    {
        LifecycleWire.Require(payload.Length >= 106 && payload[35] == 0 && (payload[34] & 0xfc) == 0);
        var hook = (int)LifecycleWire.Get(payload, 100, 2); var wasm = LifecycleWire.Get(payload, 102, 4);
        var clear = (payload[34] & 2) != 0; var offset = 106; var length = 0; byte[]? programInterface = null;
        LifecycleWire.Require(((payload[34] & 1) == 0) == (hook == 0));
        if (clear || (ulong)hook + wasm != (ulong)(payload.Length - 106))
        {
            LifecycleWire.Require(payload.Length >= 110); var size = LifecycleWire.Get(payload, 106, 4);
            LifecycleWire.Require(size <= 952 && (size > 0 || clear) && 110 + (ulong)hook + size + wasm == (ulong)payload.Length);
            offset = 110; length = (int)size; programInterface = payload[(offset + hook)..(offset + hook + length)];
        }
        var value = new NativeProgramUpgrade(payload[..32], (ushort)LifecycleWire.Get(payload, 32, 2), payload[36..68], payload[68..100],
            payload[offset..(offset + hook)], clear, programInterface, payload[(offset + hook + length)..]);
        LifecycleWire.Require(value.Encode().SequenceEqual(payload)); return value;
    }
}

public sealed class NativeProgramWindDown : INativeProgramLifecycle
{
    private readonly byte[] _payload;
    private NativeProgramWindDown(byte[] payload) { _payload = payload.ToArray(); }
    public ushort Ordinal => 7;
    public byte Operation => _payload[32];
    public byte[] Encode() => _payload.ToArray();
    public static NativeProgramWindDown Route(byte[] program, byte[] account, byte[] asset, byte[] destination, byte[] seed)
    {
        LifecycleWire.Bytes32(program); LifecycleWire.Bytes32(account); LifecycleWire.Bytes32(asset); LifecycleWire.Bytes32(destination);
        LifecycleWire.Require(seed.Length <= 128); using var output = new MemoryStream();
        output.Write(program); output.WriteByte(1); output.Write(account); output.Write(asset); output.Write(destination);
        LifecycleWire.Put(output, (ulong)seed.Length, 2); output.Write(seed); return Decode(output.ToArray());
    }
    public static NativeProgramWindDown Deprecate(byte[] program, byte[] exitProgram, ulong deadlineBatch)
    {
        LifecycleWire.Bytes32(program); LifecycleWire.Bytes32(exitProgram); using var output = new MemoryStream();
        output.Write(program); output.WriteByte(2); output.Write(exitProgram); LifecycleWire.Put(output, deadlineBatch, 8); return Decode(output.ToArray());
    }
    public static NativeProgramWindDown Tombstone(byte[] program)
    {
        LifecycleWire.Bytes32(program); return Decode([.. program, 3]);
    }
    public static NativeProgramWindDown Exit(byte[] program, byte[] account)
    {
        LifecycleWire.Bytes32(program); LifecycleWire.Bytes32(account); return Decode([.. program, 4, .. account]);
    }
    public static NativeProgramWindDown Decode(byte[] payload)
    {
        LifecycleWire.Require(payload.Length >= 33 && payload[..32].Any(value => value != 0));
        switch (payload[32])
        {
            case 1:
                LifecycleWire.Require(payload.Length >= 131); var seed = LifecycleWire.Get(payload, 129, 2);
                LifecycleWire.Require(seed <= 128 && (ulong)payload.Length == 131 + seed); break;
            case 2: LifecycleWire.Require(payload.Length == 73); break;
            case 3: LifecycleWire.Require(payload.Length == 33); break;
            case 4: LifecycleWire.Require(payload.Length == 65); break;
            default: throw new ArgumentException("Wind-down operation");
        }
        return new NativeProgramWindDown(payload);
    }
}

public sealed class NativeProgramLifecycleRequest
{
    private readonly byte[] _payload;
    private readonly byte[] _signed;
    public ushort Ordinal { get; }
    public byte[] Payload => _payload.ToArray();
    public byte[] SignedActivity => _signed.ToArray();
    public byte[] ActivityId => LifecycleWire.Hash(Encoding.UTF8.GetBytes("LXP/v1/activity-id\0"), _signed);
    public string IdempotencyKey { get; }
    public NativeProgramLifecycleRequest(INativeProgramLifecycle operation, byte[] signedActivity)
    {
        Ordinal = operation.Ordinal; _payload = LifecycleWire.Decode(Ordinal, operation.Encode()).Encode(); _signed = signedActivity.ToArray();
        IdempotencyKey = Convert.ToHexString(LifecycleWire.Bind(Ordinal, _payload, _signed)).ToLowerInvariant();
    }
}

internal static class LifecycleWire
{
    internal static void Require(bool valid) { if (!valid) throw new ArgumentException("Non-canonical Programs lifecycle"); }
    internal static void Bytes32(byte[] value) { Require(value.Length == 32); }
    internal static byte[] Hash(params byte[][] parts)
    {
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        foreach (var part in parts) hash.AppendData(part); return hash.GetHashAndReset();
    }
    internal static void Code(byte[] program, ushort abi, byte[] hash, byte[] wasm)
    {
        Bytes32(program); Bytes32(hash);
        Require(program.Any(value => value != 0) && abi is >= 1 and <= 3 && wasm.Length is >= 8 and <= 1048576 &&
            wasm.AsSpan(0, 8).SequenceEqual(new byte[] { 0, 97, 115, 109, 1, 0, 0, 0 }) && CryptographicOperations.FixedTimeEquals(hash, SHA256.HashData(wasm)));
    }
    internal static ulong Get(byte[] bytes, int offset, int length)
    {
        Require(offset >= 0 && length >= 0 && offset <= bytes.Length - length); ulong result = 0;
        foreach (var value in bytes.AsSpan(offset, length)) result = (result << 8) | value; return result;
    }
    internal static void Put(Stream output, ulong value, int length)
    {
        Span<byte> bytes = stackalloc byte[8]; BinaryPrimitives.WriteUInt64BigEndian(bytes, value); output.Write(bytes[(8 - length)..]);
    }
    internal static INativeProgramLifecycle Decode(ushort ordinal, byte[] payload) => ordinal switch
    {
        1 => NativeProgramDeploy.Decode(payload),
        2 => NativeProgramUpgrade.Decode(payload),
        7 => NativeProgramWindDown.Decode(payload),
        _ => throw new ArgumentException("Programs lifecycle ordinal"),
    };
    internal static byte[] Bind(ushort ordinal, byte[] expected, byte[] signed)
    {
        Require(signed.Length is > 0 and <= 1048576); var offset = 0;
        ulong Read(int length) { var value = Get(signed, offset, length); offset += length; return value; }
        byte[] Bounded(int maximum)
        {
            var length = Read(4); Require(length <= (ulong)maximum && length <= (ulong)(signed.Length - offset));
            var value = signed[offset..(offset + (int)length)]; offset += (int)length; return value;
        }
        Require(Read(2) == 3 && Read(2) == 0x1001 && Read(1) == 12 && Read(1) == 1 && Read(2) == 3 && Read(1) == 2);
        Read(4); Require(Read(1) == 3 && Read(4) == (0x00090000UL | ordinal) && Read(1) == 4);
        Bounded(255); Require(Read(1) == 5); Bounded(524288); Require(Read(1) == 6); Read(8); Require(Read(1) == 7);
        var before = Read(8); var after = Read(8); Require(after >= before && Read(1) == 8);
        var key = Bounded(32); Require(key.Length == 32 && Read(1) == 9); Read(8); Read(8); Require(Read(1) == 10);
        var hash = Bounded(32); Require(hash.Length == 32 && Read(1) == 11); var payload = Bounded(524288);
        Require(Read(1) == 12); Bounded(128);
        Require(offset == signed.Length && payload.SequenceEqual(expected) && hash.SequenceEqual(Hash(Encoding.UTF8.GetBytes("LXP/v1/payload-hash\0"), payload)));
        return key;
    }
}
