#!/usr/bin/env bash
# Download real long-form fixtures listed in sources.tsv into fixtures/media/ and verify sha1.
# All are CC BY 3.0 from Wikimedia Commons (license-reviewed). Attribution is in sources.tsv
# and must accompany any published eval results or clips made from them.
# Usage: ./fetch.sh            # smallest one (danaylov, 137 MB)
#        ./fetch.sh all        # all five (~1.8 GB)
#        ./fetch.sh olave fox  # by id
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p media
want=("$@"); [[ ${#want[@]} -eq 0 ]] && want=(danaylov)
while IFS=$'\t' read -r id url bytes sha1 dur license attribution title; do
  [[ "$id" == \#* || -z "$id" ]] && continue
  if [[ "${want[0]}" != all ]] && [[ ! " ${want[*]} " =~ " $id " ]]; then continue; fi
  out="media/$id.webm"
  if [[ -f "$out" ]] && [[ "$(stat -f%z "$out")" == "$bytes" ]]; then
    echo "$id: present"; continue
  fi
  echo "$id: downloading $((bytes / 1000000)) MB ($license, $attribution)"
  curl -fL --retry 3 -C - -A "choco-video-fixtures/0.1" -o "$out" "$url"
  got="$(shasum -a 1 "$out" | awk '{print $1}')"
  [[ "$got" == "$sha1" ]] || { echo "$id: sha1 mismatch ($got != $sha1)" >&2; rm -f "$out"; exit 1; }
  echo "$id: ok ${dur}s"
done < sources.tsv
