#nullable enable

using System.Diagnostics;
using System.Globalization;
using System.Text.Json;

namespace LayerX.Sdk;

public sealed record MirrorCandidate(int Source, byte[] Commitment);
public sealed record MirrorPolicy(MirrorPolicyKind Kind, IReadOnlyList<MirrorCandidate> Candidates, int Minimum = 1);
public sealed record MirrorVerification(string Level, ulong BatchNumber, byte[] HeaderDigest,
    byte[] EvidenceDigest, string SourceId, string Target, string CanonicalPosition,
    string Provenance, ulong? LatestBatch, string BatchLag, int FailoverCount,
    int AgreeingSources, string CheckpointLevel);
public sealed class MirrorVerificationException(string code) : Exception($"mirror verification refused: {code}") { public string Code { get; } = code; }

public sealed class MirrorSourceVerifier
{
    private readonly string executable;
    private readonly string configuration;
    private readonly TimeSpan timeout;
    public MirrorSourceVerifier(string executable, string configuration, TimeSpan timeout)
    {
        if (!Path.IsPathFullyQualified(executable) || !Path.IsPathFullyQualified(configuration) || timeout < TimeSpan.FromMilliseconds(100) || timeout > TimeSpan.FromSeconds(120)) throw new MirrorVerificationException("configuration");
        this.executable=executable;this.configuration=configuration;this.timeout=timeout;
    }
    public Task<MirrorVerification> VerifyReceiptAsync(ulong batch, MirrorPolicy policy, ReadOnlyMemory<byte> receipt, CancellationToken cancellation = default) =>
        VerifyAsync(batch,policy,new Dictionary<string,object>{{"kind","receipt"},{"canonical_hex",Convert.ToHexString(receipt.Span).ToLowerInvariant()}},cancellation);
    public Task<MirrorVerification> VerifyStateAsync(ulong batch, MirrorPolicy policy, ReadOnlyMemory<byte> state, ReadOnlyMemory<byte> proof, CancellationToken cancellation = default) =>
        VerifyAsync(batch,policy,new Dictionary<string,object>{{"kind","state"},{"canonical_hex",Convert.ToHexString(state.Span).ToLowerInvariant()},{"proof_hex",Convert.ToHexString(proof.Span).ToLowerInvariant()}},cancellation);
    private async Task<MirrorVerification> VerifyAsync(ulong batch, MirrorPolicy policy, Dictionary<string,object> evidence, CancellationToken cancellation)
    {
        if(batch==0||policy.Candidates.Count==0||policy.Candidates.Count>MirrorSchemaV2.MaximumSources)throw new MirrorVerificationException("configuration");
        var seen=new HashSet<int>();var candidates=new List<object>();foreach(var value in policy.Candidates){if(value.Source<0||!seen.Add(value.Source)||value.Commitment.Length!=32)throw new MirrorVerificationException("configuration");candidates.Add(new{source=value.Source,commitment_hex=Convert.ToHexString(value.Commitment).ToLowerInvariant()});}
        object wirePolicy=policy.Kind switch {MirrorPolicyKind.Exact when candidates.Count==1=>new{kind="exact",candidate=candidates[0]},MirrorPolicyKind.OrderedPreference=>new{kind="ordered-preference",candidates},MirrorPolicyKind.Agreement when policy.Minimum>0&&policy.Minimum<=candidates.Count=>new{kind="agreement",candidates,minimum=policy.Minimum},_=>throw new MirrorVerificationException("configuration")};
        var request=JsonSerializer.SerializeToUtf8Bytes(new{batch_number=batch.ToString(CultureInfo.InvariantCulture),evidence,policy=wirePolicy});if(request.Length>40*1024*1024)throw new MirrorVerificationException("bounds");
        var start=new ProcessStartInfo(executable){RedirectStandardInput=true,RedirectStandardOutput=true,RedirectStandardError=true,UseShellExecute=false};start.ArgumentList.Add(configuration);using var process=new Process{StartInfo=start};
        try{if(!process.Start())throw new MirrorVerificationException("unavailable");await process.StandardInput.BaseStream.WriteAsync(request,cancellation);process.StandardInput.Close();using var deadline=CancellationTokenSource.CreateLinkedTokenSource(cancellation);deadline.CancelAfter(timeout);var outputTask=process.StandardOutput.ReadToEndAsync(deadline.Token);await process.WaitForExitAsync(deadline.Token);var output=await outputTask;if(output.Length>1_048_576)throw new MirrorVerificationException("bounds");using var document=JsonDocument.Parse(output,new JsonDocumentOptions{MaxDepth=32});var root=document.RootElement;if(!root.TryGetProperty("ok",out var ok)||!ok.GetBoolean())throw new MirrorVerificationException(root.TryGetProperty("error",out var error)?error.GetString()??"unavailable":"unavailable");var value=root.GetProperty("verification");ulong? latestBatch=null;if(value.TryGetProperty("latestBatch",out var latest)&&latest.ValueKind!=JsonValueKind.Null)latestBatch=Unsigned(latest);return new MirrorVerification(value.GetProperty("level").GetString()??throw new MirrorVerificationException("malformed"),Unsigned(value.GetProperty("batchNumber")),Digest(value,"headerDigest"),Digest(value,"evidenceDigest"),Text(value,"sourceId"),Text(value,"target"),Text(value,"canonicalPosition"),Text(value,"provenance"),latestBatch,Text(value,"batchLag"),value.GetProperty("failoverCount").GetInt32(),value.GetProperty("agreeingSources").GetInt32(),Text(value,"checkpointLevel"));}
        catch(OperationCanceledException){if(!process.HasExited)process.Kill(true);throw new MirrorVerificationException("unavailable");}
        catch(JsonException error){throw new MirrorVerificationException(error.Path is null?"malformed":"malformed");}
    }
    private static string Text(JsonElement value,string name)=>value.GetProperty(name).GetString()??throw new MirrorVerificationException("malformed");
    private static ulong Unsigned(JsonElement value){if(value.ValueKind!=JsonValueKind.String)throw new MirrorVerificationException("malformed");var text=value.GetString();if(string.IsNullOrEmpty(text)||text[0]=='0'||!ulong.TryParse(text,NumberStyles.None,CultureInfo.InvariantCulture,out var result)||result==0||result.ToString(CultureInfo.InvariantCulture)!=text)throw new MirrorVerificationException("malformed");return result;}
    private static byte[] Digest(JsonElement value,string name){try{var result=Convert.FromHexString(Text(value,name));if(result.Length!=32)throw new MirrorVerificationException("malformed");return result;}catch(FormatException){throw new MirrorVerificationException("malformed");}}
}
