#!/usr/bin/env sh
set -eu

cargo check
cargo run -- check examples/find_max_even.jb --json-batch
cargo run -- check examples/type_error.jb --json-batch || true
cargo run -- check examples/project --json-batch
cargo run -- build examples/project --emit=ast-json
cargo run -- build examples/project --emit=ir
cargo run -- build examples/linux_main.jb --emit=exe -o /tmp/jbc-linux-main
set +e
/tmp/jbc-linux-main
exit_code=$?
set -e
test "$exit_code" -eq 7
cargo run -- build examples/native_calls.jb --emit=exe -o /tmp/jbc-native-calls
set +e
/tmp/jbc-native-calls
exit_code=$?
set -e
test "$exit_code" -eq 5
cargo run -- build examples/native_condition.jb --emit=exe -o /tmp/jbc-native-condition
set +e
/tmp/jbc-native-condition
exit_code=$?
set -e
test "$exit_code" -eq 42
cargo run -- build examples/find_max_even.jb --emit=exe --entry find_max_even -o /tmp/jbc-native-loop
set +e
/tmp/jbc-native-loop
exit_code=$?
set -e
test "$exit_code" -eq 255
cargo run -- build examples/native_runtime.jb --emit=exe -o /tmp/jbc-native-runtime
set +e
runtime_output=$(/tmp/jbc-native-runtime 12)
exit_code=$?
set -e
test "$exit_code" -eq 19
test "$runtime_output" = "19"
cargo run -- build examples/native_string.jb --emit=exe -o /tmp/jbc-native-string
set +e
string_output=$(/tmp/jbc-native-string 23)
exit_code=$?
set -e
test "$exit_code" -eq 23
test "$string_output" = "hello from JapalityBean"
cargo run -- build examples/native_vector.jb --emit=exe -o /tmp/jbc-native-vector
set +e
vector_output=$(/tmp/jbc-native-vector 4)
exit_code=$?
set -e
test "$exit_code" -eq 10
test "$vector_output" = "10"
