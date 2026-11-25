#!/usr/bin/env bash
# nextest wrapper script (.config/nextest.toml): runs one test process and
# records its CPU time, children included, for scripts/check_test_budgets.py.
# CPU time, unlike wall time, does not grow when other work loads the machine.
# Appends "binary-id<TAB>test-name<TAB>ms" to
# target/nextest/<profile>/cpu/<run-id>.tsv; one short write per test, so
# parallel tests do not mix their lines.
"$@"
status=$?
# Only builtins from here on, so that nothing else counts as the test's
# children; `mkdir` runs once per run.
export LC_ALL=C # `times` writes the locale's decimal point
ms() { # 1m2.345s -> 62345
  local m=${1%%m*} s=${1#*m}
  s=${s%s}
  echo $((10#$m * 60000 + 10#${s%.*} * 1000 + 10#${s#*.}))
}
dir="$NEXTEST_WORKSPACE_ROOT/target/nextest/$NEXTEST_PROFILE/cpu"
[ -d "$dir" ] || mkdir -p "$dir"
# `times` prints the shell's user and system time, then its children's
# ("0m0.012s 0m0.004s"). It must run in this shell, not in a subshell.
times >"$dir/$$.times"
{ read -r _ && read -r user sys; } <"$dir/$$.times"
printf '%s\t%s\t%s\n' "$NEXTEST_BINARY_ID" "$NEXTEST_TEST_NAME" "$(($(ms "$user") + $(ms "$sys")))" >>"$dir/$NEXTEST_RUN_ID.tsv"
# `rm` last: it would count as a child above.
rm -f "$dir/$$.times"
# A test that died of a signal dies of it again, so nextest names the signal.
if ((status > 128)); then
  trap - $((status - 128))
  kill -s $((status - 128)) $$
fi
exit $status
