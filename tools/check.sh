#!/bin/sh
# Runs tools/check.ps1 from Git Bash or WSL. Arguments (-Fast, -Quick, -Smoke) pass through.
dir=$(cd "$(dirname "$0")" && pwd)
if command -v wslpath >/dev/null 2>&1; then
    script=$(wslpath -w "$dir/check.ps1")
    ps=powershell.exe
else
    script=$(cygpath -w "$dir/check.ps1" 2>/dev/null || echo "$dir/check.ps1")
    ps=powershell.exe
fi
exec "$ps" -NoProfile -ExecutionPolicy Bypass -File "$script" "$@"
