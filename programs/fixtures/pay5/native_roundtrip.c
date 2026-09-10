#define main pay5_historical_main
#include "../../../tests/programs/test_call_activity.c"
#undef main
#include "native_roundtrip.h"

int main(int argc, char **argv)
{
    if (argc == 2 && strcmp(argv[1], "--token") == 0)
        return pay5_token_roundtrip();
    if (argc == 2 && strcmp(argv[1], "--merchant") == 0)
        return pay5_merchant_roundtrip();
    return 1;
}
