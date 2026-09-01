#include <stdbool.h>

#include "binaryen/src/binaryen-c.h"

#ifdef __cplusplus
extern "C" {
#endif

BinaryenModuleRef BinaryenModuleSafeRead(const char* input, size_t inputSize);

BinaryenModuleRef translateToFuzz(const char *data, size_t len, bool emitAtomics);

void BinaryenShimDisposeBinaryenModuleAllocateAndWriteResult(
    BinaryenModuleAllocateAndWriteResult result
);

void BinaryenModuleRunPassesWithSettings(
    BinaryenModuleRef module, const char** passes, BinaryenIndex numPasses,
    int shrinkLevel, int optimizeLevel, int debugInfo
);

int BinaryenModuleSafeValidate(BinaryenModuleRef module);

#ifdef __cplusplus
}
#endif
