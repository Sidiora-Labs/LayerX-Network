using System.Numerics;
using System.Runtime.CompilerServices;
using System.Text.Json;
using LayerX.Sdk;
using Xunit;
using static LayerX.Sdk.NativeCapabilitySet;

namespace LayerX.Sdk.Tests;

public sealed class NativeCapabilitySetTests
{
    private static byte[] Field(JsonElement value, string name) => Convert.FromHexString(value.GetProperty(name).GetString()!);
    private static List<Capability> LogicalGrants(JsonElement values)
    {
        var grants = new List<Capability>();
        foreach (var value in values.EnumerateArray())
        {
            Capability grant = value.GetProperty("kind").GetString() switch
            {
                "StorageRead" => new StorageRead(),
                "StorageWrite" => new StorageWrite(),
                "EmitEvent" => new EmitEvent(),
                "Call" => new Call(Field(value, "program")),
                "Transfer402" => new Transfer402(Field(value, "asset"), Field(value, "to"), BigInteger.Parse(value.GetProperty("maximum_amount").GetString()!, System.Globalization.CultureInfo.InvariantCulture)),
                "ProgramSpend" => new ProgramSpend(Field(value, "owner_program"), Field(value, "seed"), Field(value, "source_account"), Field(value, "asset"), Field(value, "to"), BigInteger.Parse(value.GetProperty("maximum_amount").GetString()!, System.Globalization.CultureInfo.InvariantCulture)),
                "ReceiptRead" => new ReceiptRead(Field(value, "receipt_digest")),
                "BalanceView" => new BalanceView(Field(value, "account"), Field(value, "asset"), Field(value, "receipt_digest")),
                "SharedStorageRead" => new SharedStorageRead(),
                "SharedStorageWrite" => new SharedStorageWrite(),
                _ => throw new ArgumentException("Unknown fixture capability"),
            };
            Assert.Equal(value.GetProperty("tag").GetByte(), Encode([grant])[2]); grants.Add(grant);
        }
        return grants;
    }
    private static void AssertEncoding(IReadOnlyList<Capability> grants, byte[] expected)
    {
        Assert.Equal(expected, Encode(grants)); Assert.Equal(expected, Encode(Decode(expected)));
    }
    private static string FixturePath([CallerFilePath] string path = "")
    {
        var root = Path.GetDirectoryName(path)!;
        for (var depth = 0; depth < 5; depth++) root = Path.GetDirectoryName(root)!;
        return Path.Combine(root, "platform/sdk/conformance/fixtures/native-program-capabilities-v2.json");
    }
    [Fact]
    public void RuntimeFixtureBindsLogicalGrantsAndEscalations()
    {
        using var document = JsonDocument.Parse(File.ReadAllText(FixturePath())); var fixture = document.RootElement;
        Assert.Equal(new byte[] { 1, 2, 3, 4, 5, 9, 6, 10, 7, 8 }, fixture.GetProperty("capabilities").EnumerateArray().Select(value => value.GetProperty("tag").GetByte()).ToArray());
        var parent = LogicalGrants(fixture.GetProperty("capabilities")); AssertEncoding(parent, Field(fixture, "canonical_hex"));
        var requested = LogicalGrants(fixture.GetProperty("narrowed_capabilities")); AssertEncoding(requested, Field(fixture, "narrowed_hex"));
        var narrowed = Narrow(parent, requested); AssertEncoding(narrowed, Field(fixture, "narrowed_hex"));
        Assert.True(fixture.GetProperty("equal_narrowing_accepted").GetBoolean()); AssertEncoding(Narrow(parent, parent), Field(fixture, "canonical_hex"));
        Assert.Equal(3, fixture.GetProperty("escalation_cases").GetArrayLength());
        foreach (var escalation in fixture.GetProperty("escalation_cases").EnumerateArray())
        {
            Assert.Equal("narrowed", escalation.GetProperty("parent").GetString()); Assert.False(escalation.GetProperty("accepted").GetBoolean());
            var grants = LogicalGrants(escalation.GetProperty("capabilities")); AssertEncoding(grants, Field(escalation, "canonical_hex"));
            Assert.Throws<ArgumentException>(() => Narrow(narrowed, grants));
        }
    }

    private static byte[] Identifier(byte value) => Enumerable.Repeat(value, 32).ToArray();
    [Fact]
    public void BoundsAndNarrowingMatchRuntime()
    {
        var owner = Identifier(1); var asset = Identifier(2); var to = Identifier(3); var receipt = Identifier(4);
        byte[] seed = [0, 255]; var source = DeriveProgramAccount(owner, seed); var maximum = BigInteger.One << 64;
        var spend = new ProgramSpend(owner, seed, source, asset, to, maximum); var view = new BalanceView(source, asset, receipt);
        Capability[] parent = [new SharedStorageWrite(),
            new SharedStorageRead(),
            view,
            new ReceiptRead(receipt),
            spend,
            new Transfer402(asset, to, maximum),
            new Call(owner),
            new EmitEvent(),
            new StorageWrite(),
            new StorageRead()];
        var encoded = Encode(parent); Assert.Equal(new byte[] { 0, 10, 1, 2, 3 }, encoded[..5]);
        Assert.Equal(2, Narrow(parent, [spend with { MaximumAmount = maximum - 1 }, view]).Count);
        Assert.Throws<ArgumentException>(() => Narrow(parent, [spend with { MaximumAmount = maximum + 1 }]));
        var changedView = view with { ReceiptDigest = Identifier(5) };
        Assert.Throws<ArgumentException>(() => Narrow(parent, [changedView]));
        Assert.Throws<ArgumentException>(() => Encode([view, changedView]));
        Assert.Throws<ArgumentException>(() => Narrow([], [new StorageRead()]));
        Assert.Throws<ArgumentException>(() => Encode([new Transfer402(asset, to, 0)]));
        Assert.Throws<ArgumentException>(() => Encode([new Transfer402(asset, to, BigInteger.One << 128)]));
        Assert.Throws<ArgumentException>(() => Encode([new Call(new byte[32])]));
        Assert.Throws<ArgumentException>(() => Encode([spend with { SourceAccount = Identifier(9) }]));
        Assert.Throws<ArgumentException>(() => Encode([spend with { Seed = new byte[129] }]));
        for (var length = 0; length < encoded.Length; length++) Assert.Throws<ArgumentException>(() => Decode(encoded[..length]));
        Assert.Throws<ArgumentException>(() => Decode([.. encoded, 0]));
        foreach (var malformed in new byte[][] { [0, 2, 2, 1], [0, 2, 1, 1], [0, 1, 11], [0, 239] })
            Assert.Throws<ArgumentException>(() => Decode(malformed));
        var views = Enumerable.Range(1, 33).Select(index => (Capability)new BalanceView(Identifier((byte)index), asset, receipt)).ToArray();
        _ = Encode(views[..32]); Assert.Throws<ArgumentException>(() => Encode(views));
        var full = new List<Capability>();
        for (var index = 0; index <= MaximumGrants; index++)
        {
            var fullSeed = new byte[128]; fullSeed[0] = (byte)(index >> 8); fullSeed[1] = (byte)index;
            full.Add(new ProgramSpend(owner, fullSeed, DeriveProgramAccount(owner, fullSeed), asset, to, maximum));
        }
        Assert.Equal(MaximumBytes, Encode(full.GetRange(0, MaximumGrants)).Length);
        Assert.Throws<ArgumentException>(() => Encode(full));
    }
}
