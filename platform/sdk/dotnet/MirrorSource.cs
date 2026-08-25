#nullable enable

using System.Diagnostics;
using System.Globalization;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;

namespace LayerX.Sdk;

public sealed record MirrorCandidate(int Source, byte[] Commitment);
public sealed record MirrorPolicy(MirrorPolicyKind Kind, IReadOnlyList<MirrorCandidate> Candidates, int Minimum = 1);
public sealed record MirrorVerification(string Level, ulong BatchNumber, byte[] HeaderDigest,
    byte[] EvidenceDigest, string SourceId, string Target, string CanonicalPosition,
    string Provenance, ulong? LatestBatch, string BatchLag, int FailoverCount,
    int AgreeingSources, string CheckpointLevel);
public sealed class MirrorVerificationException(string code) : Exception($"mirror verification refused: {code}")
{
    public string Code { get; } = code;
}

public sealed class MirrorSourceVerifier
{
    private const int MaximumRequestBytes = 40 * 1024 * 1024;
    private const int MaximumResponseBytes = 1024 * 1024;
    private const int MaximumEvidenceBytes = (MaximumRequestBytes - 64 * 1024) / 2;
    private const long MaximumExecutableBytes = 512L * 1024 * 1024;
    private const long MaximumConfigurationBytes = 16L * 1024 * 1024;
    private static readonly HashSet<string> ErrorCodes = new(StringComparer.Ordinal)
    {
        "configuration", "unavailable", "rate-limited", "missing", "target-mismatch",
        "source-mismatch", "malformed", "bounds", "commitment", "authorization",
        "proof", "checkpoint-unavailable", "divergent", "insufficient-agreement", "reorged"
    };

    private readonly string executable;
    private readonly string configuration;
    private readonly byte[] executableDigest;
    private readonly byte[] configurationDigest;
    private readonly TimeSpan timeout;

    public MirrorSourceVerifier(string executable, string configuration, TimeSpan timeout)
    {
        if (timeout < TimeSpan.FromMilliseconds(100) || timeout > TimeSpan.FromSeconds(120))
        {
            throw new MirrorVerificationException("configuration");
        }
        var executableInput = TrustedInput(executable, true, MaximumExecutableBytes);
        var configurationInput = TrustedInput(configuration, false, MaximumConfigurationBytes);
        this.executable = executableInput.Path;
        this.configuration = configurationInput.Path;
        this.executableDigest = executableInput.Digest;
        this.configurationDigest = configurationInput.Digest;
        this.timeout = timeout;
    }

    public Task<MirrorVerification> VerifyReceiptAsync(ulong batch, MirrorPolicy policy,
        ReadOnlyMemory<byte> receipt, CancellationToken cancellation = default)
    {
        if (receipt.Length > MaximumEvidenceBytes) throw new MirrorVerificationException("bounds");
        return VerifyAsync(batch, policy, new Dictionary<string, object>
        {
            { "kind", "receipt" },
            { "canonical_hex", Convert.ToHexString(receipt.Span).ToLowerInvariant() }
        }, cancellation);
    }

    public Task<MirrorVerification> VerifyStateAsync(ulong batch, MirrorPolicy policy,
        ReadOnlyMemory<byte> state, ReadOnlyMemory<byte> proof,
        CancellationToken cancellation = default)
    {
        if (state.Length > MaximumEvidenceBytes || proof.Length > MaximumEvidenceBytes - state.Length)
        {
            throw new MirrorVerificationException("bounds");
        }
        return VerifyAsync(batch, policy, new Dictionary<string, object>
        {
            { "kind", "state" },
            { "canonical_hex", Convert.ToHexString(state.Span).ToLowerInvariant() },
            { "proof_hex", Convert.ToHexString(proof.Span).ToLowerInvariant() }
        }, cancellation);
    }

    private async Task<MirrorVerification> VerifyAsync(ulong batch, MirrorPolicy policy,
        Dictionary<string, object> evidence, CancellationToken cancellation)
    {
        if (batch == 0 || policy.Candidates.Count == 0
            || policy.Candidates.Count > MirrorSchemaV2.MaximumSources)
        {
            throw new MirrorVerificationException("configuration");
        }
        var seen = new HashSet<int>();
        var candidates = new List<object>();
        foreach (var value in policy.Candidates)
        {
            if (value.Source < 0 || !seen.Add(value.Source) || value.Commitment.Length != 32)
            {
                throw new MirrorVerificationException("configuration");
            }
            candidates.Add(new
            {
                source = value.Source,
                commitment_hex = Convert.ToHexString(value.Commitment).ToLowerInvariant()
            });
        }
        object wirePolicy = policy.Kind switch
        {
            MirrorPolicyKind.Exact when candidates.Count == 1 =>
                new { kind = "exact", candidate = candidates[0] },
            MirrorPolicyKind.OrderedPreference => new { kind = "ordered-preference", candidates },
            MirrorPolicyKind.Agreement when policy.Minimum > 0 && policy.Minimum <= candidates.Count =>
                new { kind = "agreement", candidates, minimum = policy.Minimum },
            _ => throw new MirrorVerificationException("configuration")
        };
        var request = JsonSerializer.SerializeToUtf8Bytes(new
        {
            batch_number = batch.ToString(CultureInfo.InvariantCulture),
            evidence,
            policy = wirePolicy
        });
        if (request.Length > MaximumRequestBytes) throw new MirrorVerificationException("bounds");

        RequireTrustedInputs();
        var start = new ProcessStartInfo(executable)
        {
            RedirectStandardInput = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            UseShellExecute = false
        };
        start.ArgumentList.Add(configuration);
        using var process = new Process { StartInfo = start };
        using var deadline = CancellationTokenSource.CreateLinkedTokenSource(cancellation);
        deadline.CancelAfter(timeout);
        try
        {
            if (!process.Start()) throw new MirrorVerificationException("unavailable");
            var outputTask = ReadBoundedAsync(process.StandardOutput.BaseStream,
                MaximumResponseBytes, deadline.Token);
            var errorTask = DrainAsync(process.StandardError.BaseStream, deadline.Token);
            var inputTask = WriteInputAsync(process, request, deadline.Token);
            await Task.WhenAll(inputTask, process.WaitForExitAsync(deadline.Token));
            var output = await outputTask;
            await errorTask;
            RequireTrustedInputs();
            if (process.ExitCode != 0) throw new MirrorVerificationException("unavailable");
            if (output.Exceeded) throw new MirrorVerificationException("bounds");
            return Parse(output.Bytes, batch, policy);
        }
        catch (OperationCanceledException)
        {
            if (!process.HasExited) process.Kill(true);
            throw new MirrorVerificationException("unavailable");
        }
        catch (IOException)
        {
            if (!process.HasExited) process.Kill(true);
            throw new MirrorVerificationException("unavailable");
        }
        catch (JsonException)
        {
            throw new MirrorVerificationException("malformed");
        }
        catch (InvalidOperationException)
        {
            throw new MirrorVerificationException("malformed");
        }
        catch (KeyNotFoundException)
        {
            throw new MirrorVerificationException("malformed");
        }
        catch (FormatException)
        {
            throw new MirrorVerificationException("malformed");
        }
    }

    private static async Task WriteInputAsync(Process process, byte[] request,
        CancellationToken cancellation)
    {
        await process.StandardInput.BaseStream.WriteAsync(request, cancellation);
        process.StandardInput.Close();
    }

    private static async Task<BoundedOutput> ReadBoundedAsync(Stream stream, int maximum,
        CancellationToken cancellation)
    {
        using var retained = new MemoryStream(Math.Min(maximum, 4096));
        var buffer = new byte[8192];
        var exceeded = false;
        int read;
        while ((read = await stream.ReadAsync(buffer, cancellation)) != 0)
        {
            var remaining = maximum - checked((int)retained.Length);
            var keep = Math.Min(Math.Max(remaining, 0), read);
            if (keep != 0) retained.Write(buffer, 0, keep);
            if (keep != read) exceeded = true;
        }
        return new BoundedOutput(retained.ToArray(), exceeded);
    }

    private static async Task DrainAsync(Stream stream, CancellationToken cancellation)
    {
        var buffer = new byte[8192];
        while (await stream.ReadAsync(buffer, cancellation) != 0) { }
    }

    private static MirrorVerification Parse(byte[] output, ulong requestedBatch,
        MirrorPolicy policy)
    {
        using var document = JsonDocument.Parse(output, new JsonDocumentOptions
        {
            MaxDepth = MirrorSchemaV2.MaximumJsonDepth
        });
        var root = document.RootElement;
        if (!root.TryGetProperty("ok", out var ok) || ok.ValueKind != JsonValueKind.True)
        {
            var code = root.TryGetProperty("error", out var error) && error.ValueKind == JsonValueKind.String
                ? error.GetString() ?? "malformed" : "malformed";
            throw new MirrorVerificationException(ErrorCodes.Contains(code) ? code : "malformed");
        }
        var value = root.GetProperty("verification");
        var batch = Unsigned(value.GetProperty("batchNumber"));
        var level = Text(value, "level", 64);
        var source = Text(value, "sourceId", 64);
        var target = Text(value, "target", 2048);
        var position = Text(value, "canonicalPosition", 2048);
        var provenance = Text(value, "provenance", 16);
        var lag = Text(value, "batchLag", 64);
        var checkpoint = Text(value, "checkpointLevel", 32);
        var failover = value.GetProperty("failoverCount").GetInt32();
        var agreeing = value.GetProperty("agreeingSources").GetInt32();
        if (batch != requestedBatch || (provenance != "Canonical" && provenance != "Reorged")
            || checkpoint != "unavailable" || failover < 0 || failover >= policy.Candidates.Count
            || agreeing < 1 || agreeing > policy.Candidates.Count
            || (policy.Kind == MirrorPolicyKind.Agreement && agreeing < policy.Minimum))
        {
            throw new MirrorVerificationException("malformed");
        }
        ulong? latestBatch = null;
        if (value.TryGetProperty("latestBatch", out var latest) && latest.ValueKind != JsonValueKind.Null)
        {
            latestBatch = Unsigned(latest);
        }
        return new MirrorVerification(level, batch, Digest(value, "headerDigest"),
            Digest(value, "evidenceDigest"), source, target, position, provenance,
            latestBatch, lag, failover, agreeing, checkpoint);
    }

    private sealed record TrustedMirrorInput(string Path, byte[] Digest);

    private static TrustedMirrorInput TrustedInput(string path, bool executable, long maximum)
    {
        try
        {
            if (!Path.IsPathFullyQualified(path)
                || !string.Equals(Path.GetFullPath(path), path, StringComparison.Ordinal))
            {
                throw new MirrorVerificationException("configuration");
            }
            for (var current = new FileInfo(path).Directory; current is not null; current = current.Parent)
            {
                if (!current.Exists || current.LinkTarget is not null
                    || (current.Attributes & FileAttributes.ReparsePoint) != 0)
                {
                    throw new MirrorVerificationException("configuration");
                }
                RequireProtectedMode(current.FullName);
            }
            var info = new FileInfo(path);
            if (!info.Exists || info.LinkTarget is not null
                || (info.Attributes & (FileAttributes.Directory | FileAttributes.ReparsePoint)) != 0
                || info.Length < 0 || info.Length > maximum)
            {
                throw new MirrorVerificationException("configuration");
            }
            RequireProtectedMode(path);
            if (executable && !OperatingSystem.IsWindows()
                && (File.GetUnixFileMode(path)
                    & (UnixFileMode.UserExecute | UnixFileMode.GroupExecute | UnixFileMode.OtherExecute)) == 0)
            {
                throw new MirrorVerificationException("configuration");
            }
            using var source = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read,
                64 * 1024, FileOptions.SequentialScan);
            using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
            var buffer = new byte[64 * 1024];
            long total = 0;
            int count;
            while ((count = source.Read(buffer)) != 0)
            {
                total += count;
                if (total > maximum) throw new MirrorVerificationException("configuration");
                hash.AppendData(buffer, 0, count);
            }
            info.Refresh();
            if (!info.Exists || total != info.Length) {
                throw new MirrorVerificationException("configuration");
            }
            return new TrustedMirrorInput(path, hash.GetHashAndReset());
        }
        catch (IOException)
        {
            throw new MirrorVerificationException("configuration");
        }
        catch (UnauthorizedAccessException)
        {
            throw new MirrorVerificationException("configuration");
        }
    }

    private static void RequireProtectedMode(string path)
    {
        if (OperatingSystem.IsWindows()) return;
        var unsafeModes = UnixFileMode.GroupWrite | UnixFileMode.OtherWrite;
        if ((File.GetUnixFileMode(path) & unsafeModes) != 0)
        {
            throw new MirrorVerificationException("configuration");
        }
    }

    private void RequireTrustedInputs()
    {
        var currentExecutable = TrustedInput(executable, true, MaximumExecutableBytes);
        var currentConfiguration = TrustedInput(configuration, false, MaximumConfigurationBytes);
        if (!CryptographicOperations.FixedTimeEquals(executableDigest, currentExecutable.Digest)
            || !CryptographicOperations.FixedTimeEquals(configurationDigest, currentConfiguration.Digest))
        {
            throw new MirrorVerificationException("configuration");
        }
    }

    private static string Text(JsonElement value, string name, int maximum)
    {
        var result = value.GetProperty(name).GetString();
        if (string.IsNullOrEmpty(result) || Encoding.UTF8.GetByteCount(result) > maximum)
        {
            throw new MirrorVerificationException("malformed");
        }
        return result;
    }

    private static ulong Unsigned(JsonElement value)
    {
        if (value.ValueKind != JsonValueKind.String) throw new MirrorVerificationException("malformed");
        var text = value.GetString();
        if (string.IsNullOrEmpty(text) || text[0] == '0'
            || !ulong.TryParse(text, NumberStyles.None, CultureInfo.InvariantCulture, out var result)
            || result == 0 || result.ToString(CultureInfo.InvariantCulture) != text)
        {
            throw new MirrorVerificationException("malformed");
        }
        return result;
    }

    private static byte[] Digest(JsonElement value, string name)
    {
        try
        {
            var result = Convert.FromHexString(Text(value, name, 64));
            if (result.Length != 32) throw new MirrorVerificationException("malformed");
            return result;
        }
        catch (FormatException)
        {
            throw new MirrorVerificationException("malformed");
        }
    }

    private sealed record BoundedOutput(byte[] Bytes, bool Exceeded);
}
