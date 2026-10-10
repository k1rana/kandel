#!/usr/bin/env sh
# End-to-end smoke test for the Kandel core.
#
# Starts the core against a scratch config, verifies it spawns the managed
# process, then confirms that exiting the core tears the process tree down via
# the Job Object (kill-on-job-close).
#
# Requires: a Windows host, `cargo` on PATH, and Git Bash or MSYS for the shell
# utilities.
#
# Implementation note: the core is a native Windows process. A shell PID (from
# `$!`) is an MSYS translation and `kill` cannot terminate it, so process
# control goes through `taskkill` against the real Windows PID. The core's own
# PID is read from the log it writes, which is authoritative on both sides.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

CORE="target/debug/kandel.exe"
CONFIG=".smoke-config.toml"
LOG=".smoke-core.log"

CORE_PID=""
CHILD_PID=""

cleanup() {
  [ -n "$CORE_PID" ] && taskkill //F //PID "$CORE_PID" >/dev/null 2>&1
  [ -n "$CHILD_PID" ] && taskkill //F //PID "$CHILD_PID" >/dev/null 2>&1
  rm -f "$CONFIG" "$LOG"
}
trap cleanup EXIT INT TERM

# Strips ANSI colour codes so the log can be matched with plain grep.
read_log() {
  sed 's/\x1b\[[0-9;]*m//g' "$LOG" 2>/dev/null
}

cat > "$CONFIG" <<'TOML'
["smoke"]
command = "cmd.exe"
args = ["/c", "ping -n 120 127.0.0.1 > nul"]
hide_window = true
TOML

echo "== checking the core binary =="
# Build only when the binary is missing: the test's job is to exercise the core,
# not to rebuild it, and a caller that just built avoids a redundant compile.
if [ ! -f "$CORE" ]; then
  echo "building the core…"
  cargo build -p kandel-core || { echo "FAIL: build failed"; exit 1; }
fi

echo "== starting the core (config=$CONFIG) =="
# Start it detached from the shell's job control; the Windows PID is recovered
# from the log below.
"$CORE" -f "$CONFIG" -log-file "$LOG" -log-format text &
sleep 4

# The core logs its own Windows PID, which is the id taskkill needs.
CORE_PID="$(read_log | grep -a 'starting Kandel' | grep -o 'pid=[0-9]*' | head -1 | cut -d= -f2)"
CHILD_PID="$(read_log | grep -a 'process started' | tail -1 | grep -o 'pid=[0-9]*' | cut -d= -f2)"

if [ -z "$CORE_PID" ]; then
  echo "FAIL: the core did not report a pid"
  echo "--- log ---"; read_log
  exit 1
fi
echo "core pid: $CORE_PID"

if ! tasklist 2>/dev/null | grep -q "$CORE_PID"; then
  echo "FAIL: the core is not running"
  echo "--- log ---"; read_log
  exit 1
fi

if [ -z "$CHILD_PID" ]; then
  echo "FAIL: no child pid in the log"
  echo "--- log ---"; read_log
  exit 1
fi
echo "child pid: $CHILD_PID"

if ! tasklist 2>/dev/null | grep -q "$CHILD_PID"; then
  echo "FAIL: child process $CHILD_PID is not running"
  exit 1
fi
echo "PASS: the child process is running"

echo "== stopping the core =="
# A hard kill is the point: closing the job handle must reap the tree, so the
# test does not rely on a graceful stop path.
taskkill //F //PID "$CORE_PID" >/dev/null 2>&1
CORE_PID=""
# Give the kernel a moment to close the job.
sleep 3

if tasklist 2>/dev/null | grep -q "$CHILD_PID"; then
  echo "FAIL: child $CHILD_PID survived the core exit (the job object did not kill it)"
  exit 1
fi
echo "PASS: the child was reaped with the core"

echo "== smoke test passed =="
