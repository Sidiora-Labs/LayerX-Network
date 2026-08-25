#include "layerx/lxp_daemon.h"

#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static lxp_result parse_u64(
    const char *line, const char *prefix, uint64_t *value)
{
    char *end = NULL;
    unsigned long long parsed;
    size_t prefix_length = strlen(prefix);
    if (strncmp(line, prefix, prefix_length) != 0 ||
        line[prefix_length] == '\0')
        return LXP_ERR_NON_CANONICAL;
    if (line[prefix_length] < '0' || line[prefix_length] > '9')
        return LXP_ERR_NON_CANONICAL;
    errno = 0;
    parsed = strtoull(line + prefix_length, &end, 10);
    if (errno != 0 || end == line + prefix_length ||
        (*end != '\n' && *end != '\0'))
        return LXP_ERR_NON_CANONICAL;
    *value = (uint64_t)parsed;
    return LXP_OK;
}

static lxp_result parse_workers(
    const char *line, const char *prefix, size_t *workers)
{
    uint64_t value;
    lxp_result status = parse_u64(line, prefix, &value);
    if (status != LXP_OK || value > LXP_DAEMON_MAX_WORKERS)
        return LXP_ERR_LENGTH_LIMIT;
    *workers = (size_t)value;
    return LXP_OK;
}

static lxp_result read_line(FILE *file, char line[128])
{
    size_t length;
    if (fgets(line, 128, file) == NULL) return LXP_ERR_TRUNCATED;
    length = strlen(line);
    if (length == 0U || (length == 127U && line[126] != '\n'))
        return LXP_ERR_LENGTH_LIMIT;
    return LXP_OK;
}

lxp_result lxp_daemon_config_load(
    const char *path, lxp_daemon_configuration *config)
{
    FILE *file;
    char line[128];
    uint64_t value;
    lxp_result status;
    if (path == NULL || config == NULL) return LXP_ERR_NON_CANONICAL;
    file = fopen(path, "rb");
    if (file == NULL) return LXP_ERR_IO;
    (void)memset(config, 0, sizeof(*config));
    status = read_line(file, line);
    if (status == LXP_OK && strcmp(line, "role=sequencer\n") == 0)
        config->role = LXP_DAEMON_SEQUENCER;
    else if (status == LXP_OK && strcmp(line, "role=replica\n") == 0)
        config->role = LXP_DAEMON_REPLICA;
    else if (status == LXP_OK && strcmp(line, "role=guarantor\n") == 0)
        config->role = LXP_DAEMON_GUARANTOR;
    else status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK) status = read_line(file, line);
    if (status == LXP_OK) status = parse_u64(line, "network_id=", &value);
    if (status == LXP_OK && (value == 0U || value > UINT32_MAX))
        status = LXP_ERR_WRONG_NETWORK;
    if (status == LXP_OK) config->network_id = (uint32_t)value;
    if (status == LXP_OK) status = read_line(file, line);
    if (status == LXP_OK)
        status = parse_u64(line, "start_sequence=", &config->start_sequence);
    if (status == LXP_OK) status = read_line(file, line);
    if (status == LXP_OK)
        status = parse_workers(line, "verify_workers=",
                               &config->verify_workers);
    if (status == LXP_OK) status = read_line(file, line);
    if (status == LXP_OK)
        status = parse_workers(line, "network_workers=",
                               &config->network_workers);
    if (status == LXP_OK) status = read_line(file, line);
    if (status == LXP_OK)
        status = parse_workers(line, "projection_workers=",
                               &config->projection_workers);
    if (status == LXP_OK) status = read_line(file, line);
    if (status == LXP_OK)
        status = parse_workers(line, "checkpoint_workers=",
                               &config->checkpoint_workers);
    if (status == LXP_OK) status = read_line(file, line);
    if (status == LXP_OK && strcmp(line, "serial_execution=true\n") == 0)
        config->serial_execution = true;
    else if (status == LXP_OK &&
             strcmp(line, "serial_execution=false\n") == 0)
        config->serial_execution = false;
    else if (status == LXP_OK) status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK && fgets(line, sizeof(line), file) != NULL)
        status = LXP_ERR_TRAILING_BYTES;
    if (status == LXP_OK && config->serial_execution &&
        (config->verify_workers != 0U || config->network_workers != 0U ||
         config->projection_workers != 0U ||
         config->checkpoint_workers != 0U))
        status = LXP_ERR_NON_CANONICAL;
    if (fclose(file) != 0 && status == LXP_OK) status = LXP_ERR_IO;
    return status;
}

lxp_result lxp_daemon_config(
    const char *path, lxp_daemon_configuration *config)
{
    return lxp_daemon_config_load(path, config);
}

lxp_result lxp_daemon_role(
    const lxp_daemon_configuration *config,
    lxp_daemon_role_kind *role)
{
    if (config == NULL || role == NULL ||
        config->role < LXP_DAEMON_SEQUENCER ||
        config->role > LXP_DAEMON_GUARANTOR)
        return LXP_ERR_NON_CANONICAL;
    *role = config->role;
    return LXP_OK;
}
