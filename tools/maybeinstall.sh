sudo apt-get update
# mkfs.vfat is provided by dosfstools (required by Makefile ISO build).
sudo apt-get install -y \
  rustup \
  autoconf \
  automake \
  mtools \
  dosfstools \
  nasm \
  xorriso \
  gdb \
  qemu-system-x86 \

sudo apt-get install -y qemu-system

rustup toolchain install nightly --profile minimal \
  --component rust-src,rustfmt,clippy,rust-analyzer,llvm-tools-preview

