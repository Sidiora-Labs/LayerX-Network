#include "../../cmd/layerxd/lxp_daemon_finality_authority.c"

int main(void)
{
    static const char *const valid[] = {
        "0", "1", "1005", "-10", "0.1", "-0.2", "1e3", "1E-3", "1e+3"
    };
    static const char *const invalid[] = {
        "", "-", "01", "-01", "+1", "1.", ".1", "1e", "1e+", "NaN", "Infinity", "1x"
    };
    static const char *const documents[] = {
        "{\"id\":1,\"result\":{\"blockTimestamp\":1005,\"removed\":false,\"contractAddress\":null}}",
        "{\"id\":1,\"id\":1}", "{\"result\":01}", "{\"result\":1e+}"
    };
    json_token tokens[64];
    size_t i;
    for (i = 0U; i < sizeof(valid) / sizeof(valid[0]); ++i)
        if (!json_number(valid[i], strlen(valid[i]))) return 1;
    for (i = 0U; i < sizeof(invalid) / sizeof(invalid[0]); ++i)
        if (json_number(invalid[i], strlen(invalid[i]))) return 1;
    for (i = 0U; i < sizeof(documents) / sizeof(documents[0]); ++i) {
        json_document doc = {tokens, 0U, documents[i], documents[i] + strlen(documents[i])};
        int status = parse_value(&doc, 0U);
        if ((i == 0U && (status != 0 || doc.cursor != doc.end)) || (i != 0U && status == 0)) return 1;
    }
    (void)puts("finality JSON number grammar and duplicate-key rejection passed");
    return 0;
}
