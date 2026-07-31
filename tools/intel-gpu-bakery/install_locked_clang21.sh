#!/usr/bin/env bash
set -euo pipefail

if [[ "$#" -ne 1 || -z "$1" ]]; then
  echo "usage: $0 INSTALL_ROOT" >&2
  exit 2
fi

install_root="$1"
package_root="${install_root}/.packages"
mkdir -p "${package_root}"

while read -r name sha512 url; do
  package_path="${package_root}/${name}.deb"
  curl --fail --location --silent --show-error --retry 3 \
    --output "${package_path}" "${url}"
  printf '%s  %s\n' "${sha512}" "${package_path}" | sha512sum --check --status
  dpkg-deb --extract "${package_path}" "${install_root}"
done <<'PACKAGES'
clang-21 464dcf96c2532fe7e8520f5518a2b34cd9caf40865fb426d33663308e8b5d2e2fbdfc5d1f16fee1ec3d4bbfd06f7a2cd67068640b5abece6c1e6acab563db915 https://launchpad.net/ubuntu/+archive/primary/+files/clang-21_21.1.8-6ubuntu1_amd64.deb
libclang-cpp21 c94c3cab82c4ad4114e26b58f85a4d6d4f183041eb36cbebf7225a5cdc8adf5779b925462b343886e23821db5a2bc2c9d2c0df70fb2885acf6f6f3cf71bb2b30 https://launchpad.net/ubuntu/+archive/primary/+files/libclang-cpp21_21.1.8-6ubuntu1_amd64.deb
libllvm21 40586a16a0e965fb4d7533caaf384b06fa2dcb8b9c0f7092fe6f3809c9c70a33d1ceb9dbeb2c569be824f89032d6c7f787d43add23b94e9185e74e5861cfb916 https://launchpad.net/ubuntu/+archive/primary/+files/libllvm21_21.1.8-6ubuntu1_amd64.deb
libclang-common-21-dev 4e0ce4865a4ee3f8d8a762fe276e797a11bafa46bb67a735a30da0af0ef9d3090a11266f6aaec4529219dfd91b48be25ec34acfb6a9b2ab711316afd98aa73db https://launchpad.net/ubuntu/+archive/primary/+files/libclang-common-21-dev_21.1.8-6ubuntu1_amd64.deb
libclang-rt-21-dev c388803b7072d6236418bd30d57f16eeaf13d78a0c057426d7f7555014780a36ec8de342bb417ac9a7e7539003de5365c0eeecdc99c36f8a6c66eb5ec7b0d7ba https://launchpad.net/ubuntu/+archive/primary/+files/libclang-rt-21-dev_21.1.8-6ubuntu1_amd64.deb
clang-tools-21 570fc4db8e92df501e173ed77e317730c40f5063b9016c366aa2bd70b90bfd82a07f423faac043fd39f2181d8cda525b53c727e32ba43d10b0f90b7c63958d79 https://launchpad.net/ubuntu/+archive/primary/+files/clang-tools-21_21.1.8-6ubuntu1_amd64.deb
PACKAGES

test -x "${install_root}/usr/lib/llvm-21/bin/clang"
test -f "${install_root}/usr/lib/x86_64-linux-gnu/libLLVM.so.21.1"
test -f "${install_root}/usr/lib/x86_64-linux-gnu/libclang-cpp.so.21.1"
