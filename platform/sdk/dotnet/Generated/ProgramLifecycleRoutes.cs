namespace LayerX.Sdk;

internal static class ProgramLifecycleRoutes
{
    internal static readonly IReadOnlyDictionary<string, ushort> Ordinals = new Dictionary<string, ushort>
    {
        ["program.deploy"] = 1,
        ["program.upgrade"] = 2,
        ["program.wind-down"] = 7,
    };
    internal static readonly IReadOnlyDictionary<string, string> Paths = new Dictionary<string, string>
    {
        ["program.deploy"] = "/v1/programs/deploy",
        ["program.upgrade"] = "/v1/programs/upgrade",
        ["program.wind-down"] = "/v1/programs/wind-down",
    };
}
