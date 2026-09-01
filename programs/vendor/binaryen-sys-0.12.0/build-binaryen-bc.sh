set -e

echo `env`

cd binaryen

echo "Building binaryen LLVM BC"

if [ -z $OUT_DIR ]; then
  echo "$0: OUT_DIR environment variable should be set"
  exit 1
fi

if [ -z $EMSCRIPTEN ]; then
  if (which emcc >/dev/null); then
    # Found emcc in PATH -- set EMSCRIPTEN (we need this to access webidl_binder.py)
    EMSCRIPTEN=$(dirname "$(which emcc)")
  else
    echo "$0: EMSCRIPTEN environment variable is not set and emcc was not found in PATH" >&2
    exit 1
  fi
elif [ ! -d "$EMSCRIPTEN" ]; then
  echo "$0: \"$EMSCRIPTEN\" (\$EMSCRIPTEN) is not a directory" >&2
  exit 1
fi

EMCC_ARGS="-std=c++11 --memory-init-file 0"
EMCC_ARGS="$EMCC_ARGS -s ALLOW_MEMORY_GROWTH=1"
EMCC_ARGS="$EMCC_ARGS -s DISABLE_EXCEPTION_CATCHING=0" # Exceptions are thrown and caught when optimizing endless loops

"$EMSCRIPTEN/em++" \
  $EMCC_ARGS \
  src/binaryen-c.cpp \
  src/ast/ExpressionAnalyzer.cpp \
  src/ast/ExpressionManipulator.cpp \
  src/ast/LocalGraph.cpp \
  src/passes/pass.cpp \
  src/passes/CoalesceLocals.cpp \
  src/passes/CodeFolding.cpp \
  src/passes/CodePushing.cpp \
  src/passes/DeadCodeElimination.cpp \
  src/passes/DuplicateFunctionElimination.cpp \
  src/passes/ExtractFunction.cpp \
  src/passes/FlattenControlFlow.cpp \
  src/passes/I64ToI32Lowering.cpp \
  src/passes/Inlining.cpp \
  src/passes/InstrumentLocals.cpp \
  src/passes/InstrumentMemory.cpp \
  src/passes/LegalizeJSInterface.cpp \
  src/passes/LocalCSE.cpp \
  src/passes/LogExecution.cpp \
  src/passes/MemoryPacking.cpp \
  src/passes/MergeBlocks.cpp \
  src/passes/Metrics.cpp \
  src/passes/NameList.cpp \
  src/passes/OptimizeInstructions.cpp \
  src/passes/PickLoadSigns.cpp \
  src/passes/PostEmscripten.cpp \
  src/passes/Precompute.cpp \
  src/passes/PrintCallGraph.cpp \
  src/passes/Print.cpp \
  src/passes/RelooperJumpThreading.cpp \
  src/passes/RemoveImports.cpp \
  src/passes/RemoveMemory.cpp \
  src/passes/RemoveUnusedBrs.cpp \
  src/passes/RemoveUnusedModuleElements.cpp \
  src/passes/RemoveUnusedNames.cpp \
  src/passes/ReorderFunctions.cpp \
  src/passes/ReorderLocals.cpp \
  src/passes/ReReloop.cpp \
  src/passes/ConstHoisting.cpp \
  src/passes/SafeHeap.cpp \
  src/passes/SSAify.cpp \
  src/passes/SimplifyLocals.cpp \
  src/passes/Untee.cpp \
  src/passes/Vacuum.cpp \
  src/emscripten-optimizer/parser.cpp \
  src/emscripten-optimizer/simple_ast.cpp \
  src/emscripten-optimizer/optimizer-shared.cpp \
  src/wasm-emscripten.cpp \
  src/support/colors.cpp \
  src/support/safe_integer.cpp \
  src/support/bits.cpp \
  src/support/threads.cpp \
  src/asmjs/asm_v_wasm.cpp \
  src/asmjs/shared-constants.cpp \
  src/wasm/wasm.cpp \
  src/wasm/wasm-type.cpp \
  src/wasm/wasm-s-parser.cpp \
  src/wasm/wasm-binary.cpp \
  src/wasm/wasm-validator.cpp \
  src/wasm/literal.cpp \
  src/cfg/Relooper.cpp \
  -Isrc/ \
  -o $OUT_DIR/binaryen-c.bc

emar cr $OUT_DIR/libbinaryen-c.a $OUT_DIR/binaryen-c.bc
