#define _POSIX_C_SOURCE 200809L
#include "layerx/lxp_bridge_credit.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_genesis.h"
#include "layerx/lxp_kernel.h"
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

int registry_read_node(const char *path, const char *actor, uint32_t network);

static int number(const char *text, uint32_t maximum, uint32_t *out)
{
    uint32_t value = 0U;
    if (*text == '\0' || (text[0] == '0' && text[1] != '\0')) return 1;
    for (; *text; ++text) {
        if (*text < '0' || *text > '9' ||
            (value > maximum / 10U ||
             (value == maximum / 10U && (uint32_t)(*text - '0') > maximum % 10U))) return 1;
        value = value * 10U + (uint32_t)(*text - '0');
    }
    if (value > maximum) return 1;
    *out = value;
    return 0;
}

static int metadata(const char *text)
{
    size_t length = strlen(text);
    if (length == 0U || length > 32U) return 1;
    for (size_t i = 0U; i < length; ++i)
        if (!((text[i] >= 'A' && text[i] <= 'Z') ||
              (text[i] >= '0' && text[i] <= '9'))) return 1;
    return 0;
}

static int asset_valid(const char *text)
{
    bool nonzero = false;
    if (strlen(text) != 64U) return 1;
    for (size_t i = 0U; i < 64U; ++i) {
        if (!((text[i] >= '0' && text[i] <= '9') ||
              (text[i] >= 'a' && text[i] <= 'f'))) return 1;
        if (text[i] != '0') nonzero = true;
    }
    return nonzero ? 0 : 1;
}

static int profile_read(const char *path, lxp_bridge_profile *profile)
{
    struct stat info;
    size_t offset = 0U;
    int fd = open(path, O_RDONLY | O_NOFOLLOW | O_NONBLOCK);
    if (fd < 0) return 1;
    if (fstat(fd, &info) != 0 || !S_ISREG(info.st_mode) ||
        info.st_size != LXP_BRIDGE_PROFILE_BYTES) { (void)close(fd); return 1; }
    while (offset < sizeof(profile->bytes)) {
        ssize_t n = read(fd, profile->bytes + offset, sizeof(profile->bytes) - offset);
        if (n < 0 && errno == EINTR) continue;
        if (n <= 0) { (void)close(fd); return 1; }
        offset += (size_t)n;
    }
    uint8_t extra;
    int result = read(fd, &extra, 1U) == 0 &&
        lxp_bridge_profile_validate(profile) == LXP_OK ? 0 : 1;
    if (close(fd) != 0) result = 1;
    return result;
}

static int module_valid(const lxp_module_iface *module)
{
    if (module == NULL || module->module_id == 0U || module->module_id > 9U ||
        module->activity_types == NULL || module->activity_type_count == 0U ||
        module->activity_type_count > 64U) return 1;
    uint32_t previous = 0U;
    for (size_t i = 0U; i < module->activity_type_count; ++i) {
        uint32_t type = module->activity_types[i];
        if ((type >> 16U) != module->module_id || (type & 65535U) == 0U ||
            type <= previous) return 1;
        previous = type;
    }
    return 0;
}

int main(int argc, char **argv)
{
    const char *asset = NULL, *symbol = NULL, *currency = NULL, *profile_path = NULL;
    const char *socket_path = NULL, *actor = NULL;
    uint32_t decimals = 39U, network = 0U, protocol = 0U;
    unsigned seen = 0U;
    bool read_node = argc > 1 && strcmp(argv[1], "read-node") == 0;
    if (argc < 2 || (!read_node && strcmp(argv[1], "generate") != 0)) goto refused;
    for (int i = 2; i < argc; i += 2) {
        unsigned bit;
        if (i + 1 == argc) goto refused;
        const char *key = argv[i], *value = argv[i + 1];
        if (strcmp(key, "--asset") == 0) { bit = 1U; asset = value; }
        else if (strcmp(key, "--symbol") == 0) { bit = 2U; symbol = value; }
        else if (strcmp(key, "--currency") == 0) { bit = 4U; currency = value; }
        else if (strcmp(key, "--decimals") == 0) { bit = 8U; if (number(value, 38U, &decimals)) goto refused; }
        else if (strcmp(key, "--network-id") == 0) { bit = 16U; if (number(value, UINT32_MAX, &network) || network == 0U) goto refused; }
        else if (strcmp(key, "--protocol-version") == 0) { bit = 32U; if (number(value, 3U, &protocol) || protocol != 3U) goto refused; }
        else if (strcmp(key, "--custody-profile") == 0) { bit = 64U; profile_path = value; }
        else if (strcmp(key, "--socket") == 0) { bit = 128U; socket_path = value; }
        else if (strcmp(key, "--actor") == 0) { bit = 256U; actor = value; }
        else goto refused;
        if (seen & bit) goto refused;
        seen |= bit;
    }
    if (read_node) {
        if (seen != (16U | 32U | 128U | 256U) ||
            registry_read_node(socket_path, actor, network) != 0) goto refused;
        return 0;
    }
    if ((seen != 63U && seen != 127U) || asset_valid(asset) ||
        metadata(symbol) || metadata(currency)) goto refused;
    lxp_bridge_profile profile;
    if (profile_path != NULL) {
        if (profile_read(profile_path, &profile)) goto refused;
        uint32_t bound = ((uint32_t)profile.bytes[201] << 24U) |
            ((uint32_t)profile.bytes[202] << 16U) |
            ((uint32_t)profile.bytes[203] << 8U) | profile.bytes[204];
        if (bound != network) goto refused;
        for (size_t i = 0U; i < 32U; ++i) {
            char hex[3];
            (void)snprintf(hex, sizeof(hex), "%02x", profile.bytes[97U + i]);
            if (memcmp(hex, asset + i * 2U, 2U) != 0) goto refused;
        }
    }
    lxp_genesis_module_plan plan;
    const lxp_module_iface *modules[LXP_GENESIS_MODULE_TABLE_MAX];
    size_t count;
    if (lxp_genesis_module_plan_default((uint16_t)protocol,
                                        profile_path != NULL, &plan) != LXP_OK)
        goto refused;
    count = plan.count;
    for (size_t i = 0U; i < count; ++i) modules[i] = plan.modules[i];
    for (size_t i = 1U; i < count; ++i) {
        const lxp_module_iface *value = modules[i];
        size_t position = i;
        while (position != 0U &&
               modules[position - 1U]->module_id > value->module_id) {
            modules[position] = modules[position - 1U];
            --position;
        }
        modules[position] = value;
    }
    for (size_t i = 0U; i < count; ++i)
        if (module_valid(modules[i]) ||
            (i > 0U && modules[i - 1U]->module_id >= modules[i]->module_id)) goto refused;
    (void)printf("{\"schema_version\":2,\"assets\":[{\"asset\":\"%s\",\"symbol\":\"%s\",\"currency\":\"%s\",\"decimals\":%u}],\"modules\":[", asset, symbol, currency, decimals);
    for (size_t i = 0U; i < count; ++i) {
        (void)printf("%s{\"module\":%u,\"ordinals\":[", i ? "," : "", modules[i]->module_id);
        for (size_t j = 0U; j < modules[i]->activity_type_count; ++j)
            (void)printf("%s%u", j ? "," : "", modules[i]->activity_types[j] & 65535U);
        (void)printf("]}");
    }
    (void)puts("]}");
    return fflush(stdout) == 0 && !ferror(stdout) ? 0 : 1;
refused:
    (void)fputs("layerx-module-registry: refused: invalid input or unavailable canonical registry\n", stderr);
    return 1;
}
