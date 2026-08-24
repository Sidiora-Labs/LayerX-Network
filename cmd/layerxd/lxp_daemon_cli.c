#include "layerx/lxp_daemon.h"

#include <string.h>

lxp_result lxp_daemon_main(int argc, char **argv)
{
    lxp_daemon_configuration configuration;
    if (argc != 3 || argv == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (strcmp(argv[1], "--check-config") == 0)
        return lxp_daemon_config_load(argv[2], &configuration);
    if (strcmp(argv[1], "--serve") == 0)
        return lxp_daemon_serve(argv[2]);
    if (strcmp(argv[1], "--authority-replica") == 0)
        return lxp_daemon_authority_replica_serve(argv[2]);
    return LXP_ERR_NON_CANONICAL;
}
