#!/usr/bin/env bash
set -euo pipefail

app_path="${1:?Expected an app bundle path}"
output_dir="${2:?Expected an artifact directory}"
mkdir -p "$output_dir"
app_path="$(cd "$app_path" && pwd)"
output_dir="$(cd "$output_dir" && pwd)"

"$app_path/Contents/MacOS/warp-oss" >"$output_dir/startup.log" 2>&1 &
app_pid=$!
trap 'kill "$app_pid" 2>/dev/null || true' EXIT

# Allow initialization to reach the window/event loop, checking for an early crash throughout.
for tick in {1..10}; do
  if ! kill -0 "$app_pid" 2>/dev/null; then
    cat "$output_dir/startup.log"
    echo "Packaged Warp OSS exited during startup." >&2
    exit 1
  fi
  sleep 1
done

echo "Packaged Warp OSS remained running through startup." | tee "$output_dir/result.txt"
python3 - "$output_dir" <<'PY'
import pathlib
import subprocess
import sys

output = pathlib.Path(sys.argv[1])
try:
    result = subprocess.run(
        ["/usr/sbin/screencapture", "-x", str(output / "startup.png")],
        capture_output=True, text=True, timeout=15,
    )
    if result.returncode:
        (output / "screenshot-unavailable.txt").write_text(result.stderr)
except subprocess.TimeoutExpired:
    (output / "screenshot-unavailable.txt").write_text("Screen capture timed out on this runner.")
PY
