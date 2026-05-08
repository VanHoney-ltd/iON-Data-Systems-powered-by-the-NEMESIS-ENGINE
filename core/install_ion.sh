#!/bin/bash
set -e

BIN_DIR="${HOME}/.local/bin"
RELEASE_DIR="/home/ghost/iON/core/target/release"

mkdir -p "$BIN_DIR"

RELEASE_DIR="/home/ghost/iON/core/target/release"

# Build release binaries with full features if any are missing
NEEDS_BUILD=0
for bin in ion chronos helios orpheus cerberus psyche charon hermes nyx obolus plutus vigil echo aether talos styg styg_verify xwin nemesis ion_ui ion_ui_export vox; do
  if [ ! -f "${RELEASE_DIR}/${bin}" ]; then
    NEEDS_BUILD=1
    break
  fi
done

if [ "$NEEDS_BUILD" -eq 1 ]; then
  echo "Building iON release binaries..."
  cd /home/ghost/iON/core && cargo build --release --features full
fi

BINS=(
  ion
  chronos
  helios
  orpheus
  cerberus
  psyche
  charon
  hermes
  nyx
  obolus
  plutus
  vigil
  echo
  aether
  talos
  styg
  styg_verify
  xwin
  nemesis
  ion_ui
  ion_ui_export
  vox
)

for bin in "${BINS[@]}"; do
  src="${RELEASE_DIR}/${bin}"
  dst="${BIN_DIR}/${bin}"
  if [ -f "$src" ]; then
    ln -sf "$src" "$dst"
    echo "Linked ${bin} -> ${dst}"
  else
    echo "Warning: ${src} not found, skipping ${bin}"
  fi
done

echo ""
echo "iON binaries installed to ${BIN_DIR}"
echo "Make sure ${BIN_DIR} is in your PATH:"
echo '  export PATH="${HOME}/.local/bin:${PATH}"'
