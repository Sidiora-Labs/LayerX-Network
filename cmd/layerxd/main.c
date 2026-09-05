#include "layerx/lxp_daemon.h"
#include <stdio.h>

int main(int argc, char **argv)
{
    lxp_result status = lxp_daemon_main(argc, argv);
    if (status != LXP_OK)
        (void)fprintf(stderr, "layerxd: failed with result %d\n", (int)status);
    return status == LXP_OK ? 0 : 1;
}
