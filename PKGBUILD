# Maintainer: Your Name <youremail@domain.com>
pkgname="iON-Data-Systems"
pkgver=2.1.0
pkgrel=1
epoch=1
pkgdesc="iOS backup and analysis toolbox powered by the NEMESIS ENGINE"
arch=('x86_64')
url="https://github.com/ghost/iON-Data-Systems"  # TODO: replace with actual URL
license=('MIT')
depends=()
makedepends=('cargo' 'rust')
source=()
noextract=()
sha256sums=()

build() {
  cd "$startdir"
  cargo build --release --locked
}

package() {
  cd "$startdir"
  install -Dm755 "target/release/minios" "$pkgdir/usr/bin/minios"
  install -Dm755 "target/release/minios-export" "$pkgdir/usr/bin/minios-export"
  install -Dm755 "target/release/pcr-packet" "$pkgdir/usr/bin/pcr-packet"
  
  # Wrapper for nemesis-hunt
  install -Dm755 <(cat <<'EOF_WRAPPER'
#!/bin/sh
exec minios nemesis "$@"
EOF_WRAPPER) "$pkgdir/usr/bin/nemesis-hunt"
}
