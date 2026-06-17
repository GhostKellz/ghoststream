# Maintainer: GhostKellz <ghost@ghostkellz.sh>
pkgname=ghoststream
pkgver=1.0.0
pkgrel=1
pkgdesc="GPU-accelerated Capture & Encode Engine - NVENC/QSV/AMF streaming"
arch=('x86_64')
url="https://github.com/ghostkellz/ghoststream"
license=('MIT')
depends=('glibc' 'pipewire' 'ffmpeg')
makedepends=('rust' 'cargo')
optdepends=(
    'nvidia-utils: NVENC hardware encoding'
    'intel-media-driver: Intel QSV encoding'
    'libva-mesa-driver: AMD AMF encoding'
    'obs-studio: OBS integration'
)
source=("$pkgname-$pkgver.tar.gz::$url/archive/v$pkgver.tar.gz")
sha256sums=('SKIP')

build() {
    cd "$pkgname-$pkgver"
    cargo build --release
}

package() {
    cd "$pkgname-$pkgver"

    # CLI binary
    install -Dm755 target/release/ghoststream "$pkgdir/usr/bin/ghoststream"

    # Documentation
    install -Dm644 README.md "$pkgdir/usr/share/doc/$pkgname/README.md"
    install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
}
