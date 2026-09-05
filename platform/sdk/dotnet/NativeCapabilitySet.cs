using System.Buffers.Binary;
using System.Diagnostics.CodeAnalysis;
using System.Numerics;
using System.Security.Cryptography;
using System.Text;

namespace LayerX.Sdk;

public static class NativeCapabilitySet
{
    public const int MaximumGrants = 238;
    public const int MaximumBytes = 65452;
    public const int MaximumBalanceViews = 32;

    public abstract record Capability;
    public sealed record StorageRead() : Capability;
    public sealed record StorageWrite() : Capability;
    public sealed record EmitEvent() : Capability;
    public sealed record Call(byte[] Program) : Capability;
    public sealed record Transfer402(byte[] Asset, byte[] To, BigInteger MaximumAmount) : Capability;
    public sealed record ProgramSpend(byte[] OwnerProgram, byte[] Seed, byte[] SourceAccount, byte[] Asset,
        byte[] To, BigInteger MaximumAmount) : Capability;
    public sealed record ReceiptRead(byte[] ReceiptDigest) : Capability;
    public sealed record BalanceView(byte[] Account, byte[] Asset, byte[] ReceiptDigest) : Capability;
    public sealed record SharedStorageRead() : Capability;
    public sealed record SharedStorageWrite() : Capability;

    private sealed record Entry(int Rank, byte[][] Key, byte[] Encoded, BigInteger Maximum, byte[] Receipt);
    private static void Require([DoesNotReturnIf(false)] bool valid)
    {
        if (!valid) throw new ArgumentException("Noncanonical native capability set");
    }
    private static byte[] Nonzero(byte[] value)
    {
        Require(value is { Length: 32 } && value.Any(item => item != 0));
        return value.ToArray();
    }
    public static byte[] DeriveProgramAccount(byte[] owner, byte[] seed)
    {
        var program = Nonzero(owner);
        Require(seed is not null && seed.Length <= 128);
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        hash.AppendData(Encoding.UTF8.GetBytes("LayerX/programs/program-account/v1\0"));
        hash.AppendData(program);
        Span<byte> length = stackalloc byte[4]; BinaryPrimitives.WriteUInt32BigEndian(length, (uint)seed.Length);
        hash.AppendData(length); hash.AppendData(seed); return hash.GetHashAndReset();
    }
    private static byte[] Amount(BigInteger value)
    {
        Require(value > 0 && value < (BigInteger.One << 128));
        var raw = value.ToByteArray(isUnsigned: true, isBigEndian: true);
        var encoded = new byte[16]; raw.CopyTo(encoded, 16 - raw.Length); return encoded;
    }
    private static Entry Describe(Capability grant)
    {
        using var output = new MemoryStream();
        int rank; byte[][] key; var maximum = BigInteger.Zero; byte[] receipt = [];
        switch (grant)
        {
            case StorageRead: rank = 0; output.WriteByte(1); key = []; break;
            case StorageWrite: rank = 1; output.WriteByte(2); key = []; break;
            case EmitEvent: rank = 2; output.WriteByte(3); key = []; break;
            case Call value: rank = 3; output.WriteByte(4); key = [Nonzero(value.Program)]; break;
            case Transfer402 value:
                rank = 4; output.WriteByte(5); key = [Nonzero(value.Asset), Nonzero(value.To)]; maximum = value.MaximumAmount; break;
            case ProgramSpend value:
                rank = 5; output.WriteByte(9);
                Require(value.Seed is not null && value.Seed.Length <= 128);
                var owner = Nonzero(value.OwnerProgram); var seed = value.Seed.ToArray();
                Require(DeriveProgramAccount(owner, seed).SequenceEqual(value.SourceAccount));
                key = [owner, seed, value.SourceAccount.ToArray(), Nonzero(value.Asset), Nonzero(value.To)]; maximum = value.MaximumAmount; break;
            case ReceiptRead value: rank = 6; output.WriteByte(6); key = [Nonzero(value.ReceiptDigest)]; break;
            case BalanceView value:
                rank = 7; output.WriteByte(10); key = [Nonzero(value.Account), Nonzero(value.Asset)]; receipt = Nonzero(value.ReceiptDigest); break;
            case SharedStorageRead: rank = 8; output.WriteByte(7); key = []; break;
            case SharedStorageWrite: rank = 9; output.WriteByte(8); key = []; break;
            default: throw new ArgumentException("Unknown native capability");
        }
        for (var index = 0; index < key.Length; index++)
        {
            if (rank == 5 && index == 1) { output.WriteByte((byte)(key[index].Length >> 8)); output.WriteByte((byte)key[index].Length); }
            output.Write(key[index]);
        }
        if (rank is 4 or 5) output.Write(Amount(maximum));
        output.Write(receipt); return new(rank, key, output.ToArray(), maximum, receipt);
    }
    private static int Compare(Entry left, Entry right)
    {
        var rank = left.Rank.CompareTo(right.Rank); if (rank != 0) return rank;
        for (var index = 0; index < left.Key.Length; index++)
        {
            var order = left.Key[index].AsSpan().SequenceCompareTo(right.Key[index]);
            if (order != 0) return order;
        }
        return 0;
    }
    public static byte[] Encode(IReadOnlyList<Capability> grants)
    {
        Require(grants is not null && grants.Count <= MaximumGrants);
        var entries = grants.Select(Describe).ToList();
        Require(entries.Count(value => value.Rank == 7) <= MaximumBalanceViews);
        entries.Sort(Compare);
        using var output = new MemoryStream(); output.WriteByte((byte)(entries.Count >> 8)); output.WriteByte((byte)entries.Count);
        Entry? prior = null;
        foreach (var value in entries)
        {
            Require(prior is null || Compare(prior, value) < 0);
            output.Write(value.Encoded); prior = value;
        }
        Require(output.Length <= MaximumBytes); return output.ToArray();
    }
    public static IReadOnlyList<Capability> Decode(byte[] encoded)
    {
        Require(encoded is not null && encoded.Length is >= 2 and <= MaximumBytes);
        var cursor = new Cursor(encoded); var count = cursor.Word(); Require(count <= MaximumGrants);
        var grants = new List<Capability>(count);
        for (var index = 0; index < count; index++)
        {
            Capability grant;
            switch (cursor.Take(1)[0])
            {
                case 1: grant = new StorageRead(); break;
                case 2: grant = new StorageWrite(); break;
                case 3: grant = new EmitEvent(); break;
                case 4: grant = new Call(cursor.Take(32)); break;
                case 5: grant = new Transfer402(cursor.Take(32), cursor.Take(32), cursor.Amount()); break;
                case 6: grant = new ReceiptRead(cursor.Take(32)); break;
                case 7: grant = new SharedStorageRead(); break;
                case 8: grant = new SharedStorageWrite(); break;
                case 9:
                    var owner = cursor.Take(32); var length = cursor.Word(); Require(length <= 128);
                    grant = new ProgramSpend(owner, cursor.Take(length), cursor.Take(32), cursor.Take(32), cursor.Take(32), cursor.Amount()); break;
                case 10: grant = new BalanceView(cursor.Take(32), cursor.Take(32), cursor.Take(32)); break;
                default: throw new ArgumentException("Unknown native capability tag");
            }
            grants.Add(grant);
        }
        Require(cursor.Finished && Encode(grants).SequenceEqual(encoded)); return grants.AsReadOnly();
    }
    public static IReadOnlyList<Capability> Narrow(IReadOnlyList<Capability> parent, IReadOnlyList<Capability> requested)
    {
        var parents = Decode(Encode(parent)).Select(Describe).ToArray(); var children = Decode(Encode(requested));
        foreach (var child in children)
        {
            var value = Describe(child); var found = parents.FirstOrDefault(ancestor => Compare(ancestor, value) == 0);
            Require(found is not null && value.Maximum <= found.Maximum && value.Receipt.SequenceEqual(found.Receipt));
        }
        return children;
    }
    private sealed class Cursor(byte[] bytes)
    {
        private int _offset;
        internal bool Finished => _offset == bytes.Length;
        internal byte[] Take(int length)
        {
            Require(length >= 0 && length <= bytes.Length - _offset);
            var value = bytes[_offset..(_offset + length)]; _offset += length; return value;
        }
        internal ushort Word() => BinaryPrimitives.ReadUInt16BigEndian(Take(2));
        internal BigInteger Amount() => new(Take(16), isUnsigned: true, isBigEndian: true);
    }
}
