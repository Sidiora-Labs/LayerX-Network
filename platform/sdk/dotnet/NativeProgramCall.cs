using System.Buffers.Binary;
using System.Text;
using System.Text.RegularExpressions;

namespace LayerX.Sdk;

public sealed record NativeProgramCall(byte[] ProgramId, ushort GuestAbi, string Entrypoint, byte[] Calldata,
    byte[] Capabilities, byte[] AccessDeclaration, uint ResponseCapacity, ulong[] Resources)
{
    public byte[] Encode()
    {
        if (ProgramId.Length != 32 || ProgramId.All(value => value == 0) || GuestAbi is not (1 or 2)
            || !Regex.IsMatch(Entrypoint, "\\A[A-Za-z0-9_.]{1,128}\\z") || Calldata.Length > 1048576
            || Capabilities.Length > 65535 || AccessDeclaration.Length > 1048576 || ResponseCapacity > 1048576 || Resources.Length != 7)
            throw new ArgumentException("invalid native program call");
        var entry = Encoding.ASCII.GetBytes(Entrypoint);
        var output = new byte[106 + entry.Length + Calldata.Length + Capabilities.Length + AccessDeclaration.Length];
        ProgramId.CopyTo(output, 0); BinaryPrimitives.WriteUInt16BigEndian(output.AsSpan(32), GuestAbi);
        BinaryPrimitives.WriteUInt16BigEndian(output.AsSpan(34), (ushort)entry.Length);
        BinaryPrimitives.WriteUInt32BigEndian(output.AsSpan(36), (uint)Calldata.Length);
        BinaryPrimitives.WriteUInt16BigEndian(output.AsSpan(40), (ushort)Capabilities.Length);
        BinaryPrimitives.WriteUInt32BigEndian(output.AsSpan(42), (uint)AccessDeclaration.Length);
        BinaryPrimitives.WriteUInt32BigEndian(output.AsSpan(46), ResponseCapacity);
        for (var i = 0; i < 7; i++) BinaryPrimitives.WriteUInt64BigEndian(output.AsSpan(50 + i * 8), Resources[i]);
        var offset = 106;
        foreach (var body in new[] { entry, Calldata, Capabilities, AccessDeclaration }) { body.CopyTo(output, offset); offset += body.Length; }
        return output;
    }

    public static NativeProgramCall Decode(byte[] payload)
    {
        if (payload.Length < 106) throw new ArgumentException("invalid native program call");
        var sizes = new ulong[] { BinaryPrimitives.ReadUInt16BigEndian(payload.AsSpan(34)), BinaryPrimitives.ReadUInt32BigEndian(payload.AsSpan(36)), BinaryPrimitives.ReadUInt16BigEndian(payload.AsSpan(40)), BinaryPrimitives.ReadUInt32BigEndian(payload.AsSpan(42)) };
        if (106UL + sizes[0] + sizes[1] + sizes[2] + sizes[3] != (ulong)payload.Length) throw new ArgumentException("invalid native program call");
        var resources = new ulong[7]; for (var i = 0; i < 7; i++) resources[i] = BinaryPrimitives.ReadUInt64BigEndian(payload.AsSpan(50 + i * 8));
        var bodies = new byte[4][]; var offset = 106;
        for (var i = 0; i < 4; i++) { bodies[i] = payload.AsSpan(offset, (int)sizes[i]).ToArray(); offset += (int)sizes[i]; }
        var call = new NativeProgramCall(payload[..32], BinaryPrimitives.ReadUInt16BigEndian(payload.AsSpan(32)), Encoding.ASCII.GetString(bodies[0]), bodies[1], bodies[2], bodies[3], BinaryPrimitives.ReadUInt32BigEndian(payload.AsSpan(46)), resources);
        if (!call.Encode().SequenceEqual(payload)) throw new ArgumentException("invalid native program call");
        return call;
    }
}
