#include "layerx/lxp_daemon.h"

#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static lxp_result relay_archive_serve(const char *configuration)
{
    const char *runtime = getenv("LAYERX_RELAY_ARCHIVE_RUNTIME");
    if (configuration == NULL || configuration[0] == '\0')
        return LXP_ERR_NON_CANONICAL;
    if (runtime == NULL)
        runtime = "/opt/layerx/relay_archive/runtime.py";
    if (runtime[0] == '\0')
        return LXP_ERR_NON_CANONICAL;
    (void)execlp("python3", "python3", runtime, "--config", configuration,
                 (char *)NULL);
    return LXP_ERR_IO;
}

lxp_result lxp_daemon_main(int argc, char **argv)
{
    lxp_daemon_configuration configuration;
    if (argc != 3 || argv == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (strcmp(argv[1], "--check-config") == 0)
        return lxp_daemon_config_load(argv[2], &configuration);
    if (strcmp(argv[1], "--serve") == 0)
        return lxp_daemon_serve(argv[2]);
    if (strcmp(argv[1], "--replica") == 0)
        return lxp_daemon_replica_serve(argv[2]);
    if (strcmp(argv[1], "--authority-replica") == 0)
        return lxp_daemon_authority_replica_serve(argv[2]);
    if (strcmp(argv[1], "--relay-archive") == 0)
        return relay_archive_serve(argv[2]);
    return LXP_ERR_NON_CANONICAL;
}
