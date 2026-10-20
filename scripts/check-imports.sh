#!/usr/bin/env bash
# Windows import check (docs/TECH.md §19.9): the shipped executables and FFmpeg DLLs must run on a
# clean Windows install. Fails when any of them imports the VC++ redistributable (vcruntime140,
# msvcp140), a MinGW runtime (libgcc, libwinpthread), an FFmpeg DLL other than the six we ship, or
# vulkan-1.dll outside the delay-load table (machines without Vulkan must still start).
# The Universal CRT (api-ms-win-crt-*, ucrtbase.dll) is allowed: it is part of Windows 10 and 11,
# and Tauri's staticVCRuntime links the VC++ runtime statically but the UCRT dynamically by design.
# Uses llvm-readobj, which needs no Visual Studio environment.
# Usage: scripts/check-imports.sh <ffmpeg bin dir> <exe or dll>...
set -euo pipefail
FFBIN="$1"; shift
READOBJ="${READOBJ:-llvm-readobj}"
command -v "$READOBJ" >/dev/null || READOBJ="/c/Program Files/LLVM/bin/llvm-readobj.exe"

ours=()
for d in "$FFBIN"/*.dll; do ours+=("$(basename "$d" | tr 'A-Z' 'a-z')"); done
is_ours() { local x; for x in "${ours[@]}"; do [[ "$x" == "$1" ]] && return 0; done; return 1; }

status=0
check() { # file
  local f="$1" kind name lname
  while IFS=$'\t' read -r kind name; do
    lname="$(tr 'A-Z' 'a-z' <<<"$name")"
    case "$lname" in
      vcruntime140*.dll|msvcp140*.dll|libgcc*|libwinpthread*|libstdc++*)
        echo "FAIL $(basename "$f"): imports $name (needs a runtime Windows does not ship)"; status=1 ;;
      vulkan-1.dll)
        [[ "$kind" == DelayImport ]] || { echo "FAIL $(basename "$f"): vulkan-1.dll must be delay-loaded"; status=1; } ;;
      avcodec-*|avformat-*|avutil-*|avfilter-*|avdevice-*|swscale-*|swresample-*|postproc-*)
        is_ours "$lname" || { echo "FAIL $(basename "$f"): imports $name, not one of our FFmpeg DLLs"; status=1; } ;;
    esac
    echo "  $(basename "$f") $kind $name" >> "${IMPORTS_LOG:-/dev/null}"
  done < <("$READOBJ" --coff-imports "$f" | awk '/^(Import|DelayImport) \{/ {k=$1} /^  Name: / {print k "\t" $2}')
}
for f in "$FFBIN"/*.dll "$@"; do check "$f"; done
[[ $status == 0 ]] && echo "imports ok: $(( ${#ours[@]} + $# )) files, no VC++ redistributable or MinGW runtime, FFmpeg limited to its ${#ours[@]} DLLs"
exit $status
