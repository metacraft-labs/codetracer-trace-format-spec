#!/usr/bin/env bash
# php corpus: codetracer-php-recorder prebuilt ext/modules/codetracer.so (built 2026-09-30 19:35 for php-8.4.21).
# Recorder revision: 6cd8696ed88c0a1e8d0c53016f0248064a8739ab
# The extension dynamically links libcodetracer_trace_writer.so; we build that .so in the scratchpad
# from the static libcodetracer_trace_writer.a produced by codetracer-evm-recorder's cargo build
# (codetracer_trace_writer_nim-2759e4c6e8ff495e/out), instead of touching codetracer-trace-format-nim.
set -u
S=${FMT_EFF:?set FMT_EFF to the measurement work directory}
R=${WS:?set WS to the workspace root}/codetracer-php-recorder
PHP=/nix/store/1g63cbiva59dwz0mj8bhqfdrwqscih60-php-8.4.21/bin/php
Z=/nix/store/13id30w3rvgj24nnz34f7qrncz48zd7l-zstd-1.5.7/lib
L=$S/corpus-src/php/lib; mkdir -p $L $S/corpus/php $S/corpus-src/php/programs
[ -f $L/libcodetracer_trace_writer.so ] || gcc -shared -o $L/libcodetracer_trace_writer.so -Wl,--whole-archive \
  ${WS:?set WS to the workspace root}/codetracer-evm-recorder/target/debug/build/codetracer_trace_writer_nim-2759e4c6e8ff495e/out/libcodetracer_trace_writer.a \
  -Wl,--no-whole-archive -L$Z -lzstd -lm -lpthread -Wl,-rpath,$Z
cp $R/tests/programs/*.php $R/tests/programs/web/app.php $S/corpus-src/php/programs/
cp $S/corpus-src/php/realistic.php $S/corpus-src/php/programs/
cd $S/corpus-src/php/programs
for f in *.php; do n=${f%.php}; rm -rf $S/corpus/php/$n
  LD_LIBRARY_PATH=$L CODETRACER_ENABLED=1 CODETRACER_TRACE_DIR=$S/corpus/php/$n CODETRACER_OUTPUT_DIR=$S/corpus/php/$n \
    timeout 300 $PHP -d extension=$R/ext/modules/codetracer.so $f >$S/corpus/php/$n.log 2>&1; echo "$n rc=$?"
done
