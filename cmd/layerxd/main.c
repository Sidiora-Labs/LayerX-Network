#include "layerx/lxp_daemon.h"

int main(int argc, char **argv)
{
    return lxp_daemon_main(argc, argv) == LXP_OK ? 0 : 1;
}
