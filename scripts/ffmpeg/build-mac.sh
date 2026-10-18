#!/usr/bin/env bash
# Minimal LGPL-2.1 FFmpeg for choco.video on macOS arm64.
# Output: vendor/ffmpeg/<version>-macos-arm64/{lib,include,lib/pkgconfig}
# Shared libraries only (LGPL compliance: dynamic linking, replaceable dylibs).
# Never add --enable-gpl, --enable-nonfree, libx264, libx265, or openh264.
set -euo pipefail

VERSION="${FFMPEG_VERSION:-8.1.3}"
SHA256="${FFMPEG_SHA256:-}"   # pinned in scripts/ffmpeg/VERSIONS after first verified download
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
WORK="${FFMPEG_WORK:-$ROOT/vendor/ffmpeg/build}"
PREFIX="$ROOT/vendor/ffmpeg/${VERSION}-macos-arm64"
JOBS="$(sysctl -n hw.ncpu)"
MESON="${MESON:-meson}"
# dav1d: BSD-2-Clause AV1 decoder, built static and linked into libavcodec (no extra dylib).
DAV1D_VERSION="1.5.4"
DAV1D_SHA256="686616b7c69eb88d44459391ab25cac13b6647a3b288835c5784e71c1514a5c5"  # published by VideoLAN

if [[ -z "$SHA256" && -f "$ROOT/scripts/ffmpeg/VERSIONS" ]]; then
  SHA256="$(awk -v v="$VERSION" '$1==v {print $2}' "$ROOT/scripts/ffmpeg/VERSIONS")"
fi

mkdir -p "$WORK"
TARBALL="$WORK/ffmpeg-$VERSION.tar.xz"
if [[ ! -f "$TARBALL" ]]; then
  curl -fL --retry 3 -o "$TARBALL" "https://ffmpeg.org/releases/ffmpeg-$VERSION.tar.xz"
fi
ACTUAL="$(shasum -a 256 "$TARBALL" | awk '{print $1}')"
if [[ -n "$SHA256" && "$ACTUAL" != "$SHA256" ]]; then
  echo "sha256 mismatch for ffmpeg-$VERSION: expected $SHA256 got $ACTUAL" >&2; exit 1
fi
echo "ffmpeg-$VERSION sha256 $ACTUAL"

# --- dav1d (software AV1; VideoToolbox covers AV1 in hardware on M3 and later)
DAV1D_TAR="$WORK/dav1d-$DAV1D_VERSION.tar.xz"
DAV1D_PREFIX="$WORK/dav1d-$DAV1D_VERSION-prefix"
if [[ ! -f "$DAV1D_PREFIX/lib/libdav1d.a" ]]; then
  [[ -f "$DAV1D_TAR" ]] || curl -fL --retry 3 -o "$DAV1D_TAR" "https://downloads.videolan.org/pub/videolan/dav1d/$DAV1D_VERSION/dav1d-$DAV1D_VERSION.tar.xz"
  echo "$DAV1D_SHA256  $DAV1D_TAR" | shasum -a 256 -c -
  rm -rf "$WORK/dav1d-$DAV1D_VERSION" && tar -xf "$DAV1D_TAR" -C "$WORK"
  ( cd "$WORK/dav1d-$DAV1D_VERSION" && \
    "$MESON" setup build --prefix="$DAV1D_PREFIX" --libdir=lib --buildtype=release --default-library=static \
      -Denable_tools=false -Denable_tests=false -Denable_examples=false \
      -Dc_args="-mmacosx-version-min=14.0" -Dc_link_args="-mmacosx-version-min=14.0" && \
    ninja -C build install )
fi
export PKG_CONFIG_PATH="$DAV1D_PREFIX/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"

SRC="$WORK/ffmpeg-$VERSION"
rm -rf "$SRC" && tar -xf "$TARBALL" -C "$WORK"
cd "$SRC"

join() { local IFS=,; echo "$*"; }

DEMUXERS=(mov matroska mp3 wav aac flac ogg)
DECODERS=(h264 hevc vp8 vp9 av1 libdav1d prores mpeg4 mjpeg png
          aac aac_fixed mp3 mp3float opus vorbis flac alac ac3 eac3
          pcm_s16le pcm_s24le pcm_s32le pcm_f32le)
PARSERS=(h264 hevc vp8 vp9 av1 aac mpegaudio opus vorbis flac ac3 mjpeg png)
BSFS=(h264_mp4toannexb hevc_mp4toannexb aac_adtstoasc vp9_superframe_split extract_extradata)
ENCODERS=(h264_videotoolbox hevc_videotoolbox aac aac_at png pcm_s16le pcm_f32le)
MUXERS=(mp4 mov ipod wav image2)
FILTERS=(abuffer abuffersink buffer buffersink aresample aformat anull null format scale
         loudnorm ebur128 volume atrim trim asetpts setpts transpose hflip vflip)
HWACCELS=(h264_videotoolbox hevc_videotoolbox vp9_videotoolbox av1_videotoolbox prores_videotoolbox)

./configure \
  --prefix="$PREFIX" \
  --install-name-dir='@rpath' \
  --arch=arm64 --target-os=darwin --cc=clang \
  --enable-shared --disable-static \
  --disable-gpl --disable-nonfree \
  --disable-autodetect --enable-videotoolbox --enable-audiotoolbox --enable-libdav1d --pkg-config-flags=--static \
  --disable-programs --disable-doc --disable-debug \
  --disable-network --disable-avdevice \
  --disable-everything \
  --enable-protocol=file,pipe \
  --enable-demuxer="$(join "${DEMUXERS[@]}")" \
  --enable-decoder="$(join "${DECODERS[@]}")" \
  --enable-parser="$(join "${PARSERS[@]}")" \
  --enable-bsf="$(join "${BSFS[@]}")" \
  --enable-encoder="$(join "${ENCODERS[@]}")" \
  --enable-muxer="$(join "${MUXERS[@]}")" \
  --enable-filter="$(join "${FILTERS[@]}")" \
  --enable-hwaccel="$(join "${HWACCELS[@]}")" \
  --enable-avcodec --enable-avformat --enable-avfilter --enable-swscale --enable-swresample \
  --extra-cflags="-mmacosx-version-min=14.0" --extra-ldflags="-mmacosx-version-min=14.0"

# Refuse to continue if configure ended up GPL or nonfree.
grep -q '^#define CONFIG_GPL 0' config.h && grep -q '^#define CONFIG_NONFREE 0' config.h \
  || { echo "configure produced a GPL or nonfree build; aborting" >&2; exit 1; }

make -j"$JOBS"
rm -rf "$PREFIX" && make install

echo "--- license"; grep -E '^#define CONFIG_(GPL|NONFREE|VERSION3) ' config.h
echo "--- installed dylib sizes"; du -sh "$PREFIX"/lib/*.dylib | grep -v '\-> ' || true
du -ch "$(readlink -f "$PREFIX"/lib/libav*.[0-9]*.dylib "$PREFIX"/lib/libsw*.[0-9]*.dylib 2>/dev/null | sort -u)" 2>/dev/null | tail -1 || true
echo "PKG_CONFIG_PATH=$PREFIX/lib/pkgconfig"
