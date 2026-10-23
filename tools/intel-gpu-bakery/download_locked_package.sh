#!/usr/bin/env bash
# Shared checksum-pinned Ubuntu package download with resumable mirror fallback.
download_locked_package() {
  local algorithm="$1" digest="$2" url="$3" archive="$4"
  local partial="${archive}.part" mirror attempt status
  local relative="${url#*/ubuntu/}"
  if [[ -f "${archive}" ]] && printf '%s  %s\n' "${digest}" "${archive}" | "${algorithm}sum" --check --status; then
    return 0
  fi
  for mirror in https://archive.ubuntu.com/ubuntu https://azure.archive.ubuntu.com/ubuntu https://de.archive.ubuntu.com/ubuntu; do
    for attempt in 1 2; do
      echo "toolchain: downloading ${archive##*/} from ${mirror} (attempt ${attempt}, resuming available bytes)" >&2
      status=0
      curl --fail --location --silent --show-error \
        --connect-timeout 15 --max-time 300 --continue-at - \
        --output "${partial}" "${mirror}/${relative}" || status=$?
      if [[ -f "${partial}" ]] && printf '%s  %s\n' "${digest}" "${partial}" | "${algorithm}sum" --check --status 2>/dev/null; then
        mv "${partial}" "${archive}"
        return 0
      fi
      if [[ "${status}" -eq 0 ]]; then
        echo "toolchain: checksum mismatch for ${archive##*/}; discarding partial download" >&2
        rm -f "${partial}"
      elif [[ "${status}" -eq 33 || "${status}" -eq 36 ]]; then
        # A server that cannot resume needs a fresh transfer.
        rm -f "${partial}"
      fi
    done
  done
  echo "toolchain: could not download verified ${archive##*/} from any mirror" >&2
  return 1
}
