#!/bin/sh
# Print size and memory numbers. Never fails the build; missing artifacts are reported as n/a.
set -eu
cd "$(dirname "$0")/.."

bytes() {
	if [ -e "$1" ]; then
		python3 - "$1" <<'PY'
import os, sys
p = sys.argv[1]
if os.path.isfile(p):
    print(os.path.getsize(p))
else:
    total = 0
    for root, _, files in os.walk(p):
        for name in files:
            try:
                total += os.path.getsize(os.path.join(root, name))
            except OSError:
                pass
    print(total)
PY
	else
		echo 0
	fi
}

mb() {
	python3 - "$1" <<'PY'
import sys
n = int(sys.argv[1])
print("n/a" if n == 0 else f"{n/1024/1024:.1f} MB")
PY
}

front=$(bytes frontend/build)
maps=0
js=0
if [ -d frontend/build ]; then
	maps=$(python3 - <<'PY'
import os
total = 0
for root, _, files in os.walk("frontend/build"):
    for name in files:
        if name.endswith(".map"):
            total += os.path.getsize(os.path.join(root, name))
print(total)
PY
)
	js=$(python3 - <<'PY'
import os
total = 0
root = "frontend/build/_app"
if os.path.isdir(root):
    for r, _, files in os.walk(root):
        for name in files:
            if name.endswith(".js"):
                total += os.path.getsize(os.path.join(r, name))
print(total)
PY
)
fi

bin=$(bytes backend/target/release/ferrochat)
echo "frontend/build: $(mb "$front")"
echo "sourcemaps: $(mb "$maps")"
echo "frontend js: $(mb "$js")"
echo "release binary: $(mb "$bin")"

if [ -n "${FERROCHAT_PID:-}" ] && [ -r "/proc/${FERROCHAT_PID}/status" ]; then
	awk '/VmRSS/ {print "process rss: " $2 " kB"}' "/proc/${FERROCHAT_PID}/status"
else
	echo "process rss: n/a"
fi
