#!/usr/bin/env bash
# Minimal LGPL-2.1 FFmpeg for choco.video on Windows x64 (MSVC), mirroring build-mac.sh.
# Run in MSYS2 bash inside a Visual Studio developer environment (cl, link, lib on PATH);
# .github/workflows/ffmpeg-windows.yml does exactly that.
# Output: vendor/ffmpeg/<version>-windows-x64-minimal/{bin/*.dll, lib/*.lib, include/}
# Never add --enable-gpl, --enable-nonfree, libx264, libx265, or openh264.
set -euo pipefail

VERSION="${FFMPEG_VERSION:-8.1.3}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
WORK="${FFMPEG_WORK:-$ROOT/vendor/ffmpeg/build-win}"
PREFIX="$ROOT/vendor/ffmpeg/${VERSION}-windows-x64-minimal"
DEPS="$WORK/deps"
JOBS="$(nproc)"
SHA256="$(awk -v v="$VERSION" '$1==v {print $2}' "$ROOT/scripts/ffmpeg/VERSIONS")"
DAV1D_VERSION="1.5.4"
DAV1D_SHA256="686616b7c69eb88d44459391ab25cac13b6647a3b288835c5784e71c1514a5c5"
NVCODEC_TAG="n13.0.19.0"   # MIT, headers only; NVENC loads nvEncodeAPI64.dll from the NVIDIA driver at runtime
AMF_TAG="v1.4.36"          # MIT, headers only; AMF loads amfrt64.dll from the AMD driver at runtime

sha() { sha256sum "$1" | cut -d' ' -f1; }
fetch() { [[ -f "$2" ]] || curl -fL --retry 3 -o "$2" "$1"; }
mkdir -p "$WORK" "$DEPS/include" "$DEPS/lib/pkgconfig"

# MSYS2 ships /usr/bin/link (coreutils), which shadows MSVC's link.exe; FFmpeg's msvc toolchain needs the latter.
if [[ -e /usr/bin/link.exe ]]; then mv /usr/bin/link.exe /usr/bin/link-coreutils.exe; fi
command -v cl >/dev/null && command -v link >/dev/null || { echo "cl/link not on PATH: run inside a VS developer environment" >&2; exit 1; }

# --- FFmpeg source, pinned
TARBALL="$WORK/ffmpeg-$VERSION.tar.xz"
fetch "https://ffmpeg.org/releases/ffmpeg-$VERSION.tar.xz" "$TARBALL"
[[ "$(sha "$TARBALL")" == "$SHA256" ]] || { echo "ffmpeg sha256 mismatch" >&2; exit 1; }

# --- dav1d, static, static CRT (-MT): AV1 software decode inside avcodec
DAV1D_TAR="$WORK/dav1d-$DAV1D_VERSION.tar.xz"
fetch "https://downloads.videolan.org/pub/videolan/dav1d/$DAV1D_VERSION/dav1d-$DAV1D_VERSION.tar.xz" "$DAV1D_TAR"
[[ "$(sha "$DAV1D_TAR")" == "$DAV1D_SHA256" ]] || { echo "dav1d sha256 mismatch" >&2; exit 1; }
if [[ ! -f "$DEPS/lib/libdav1d.a" && ! -f "$DEPS/lib/dav1d.lib" ]]; then
  rm -rf "$WORK/dav1d-$DAV1D_VERSION" && tar -xf "$DAV1D_TAR" -C "$WORK"
  ( cd "$WORK/dav1d-$DAV1D_VERSION" && \
    CC=cl meson setup build --prefix="$(cygpath -m "$DEPS")" --libdir=lib --buildtype=release \
      --default-library=static -Db_vscrt=mt -Denable_tools=false -Denable_tests=false -Denable_examples=false && \
    ninja -C build install )
fi
# Meson names an MSVC static library libdav1d.a; FFmpeg's MSVC link step asks for dav1d.lib.
[[ -f "$DEPS/lib/libdav1d.a" && ! -f "$DEPS/lib/dav1d.lib" ]] && cp "$DEPS/lib/libdav1d.a" "$DEPS/lib/dav1d.lib"

# --- NVENC and AMF headers (no code is linked; both load the vendor runtime at encoder open)
NV_TAR="$WORK/nv-codec-headers-$NVCODEC_TAG.tar.gz"
fetch "https://github.com/FFmpeg/nv-codec-headers/archive/refs/tags/$NVCODEC_TAG.tar.gz" "$NV_TAR"
echo "nv-codec-headers $NVCODEC_TAG sha256 $(sha "$NV_TAR")"
rm -rf "$WORK/nvh" && mkdir -p "$WORK/nvh" && tar -xzf "$NV_TAR" -C "$WORK/nvh" --strip-components=1
make -C "$WORK/nvh" PREFIX="$DEPS" install >/dev/null
AMF_TAR="$WORK/amf-$AMF_TAG.tar.gz"
fetch "https://github.com/GPUOpen-LibrariesAndSDKs/AMF/archive/refs/tags/$AMF_TAG.tar.gz" "$AMF_TAR"
echo "AMF $AMF_TAG sha256 $(sha "$AMF_TAR")"
rm -rf "$WORK/amf" && mkdir -p "$WORK/amf" && tar -xzf "$AMF_TAR" -C "$WORK/amf" --strip-components=1 --wildcards '*/amf/public/include/*'
rm -rf "$DEPS/include/AMF" && cp -R "$WORK/amf/amf/public/include" "$DEPS/include/AMF"

export PKG_CONFIG_PATH="$DEPS/lib/pkgconfig"
SRC="$WORK/ffmpeg-$VERSION"
rm -rf "$SRC" && tar -xf "$TARBALL" -C "$WORK"
cd "$SRC"
join() { local IFS=,; echo "$*"; }

# Same allowlist as build-mac.sh; only the platform encoders and hwaccels differ.
DEMUXERS=(mov matroska mp3 wav aac flac ogg)
DECODERS=(h264 hevc vp8 vp9 av1 libdav1d prores mpeg4 mjpeg png
          aac aac_fixed mp3 mp3float opus vorbis flac alac ac3 eac3
          pcm_s16le pcm_s24le pcm_s32le pcm_f32le)
PARSERS=(h264 hevc vp8 vp9 av1 aac mpegaudio opus vorbis flac ac3 mjpeg png)
BSFS=(h264_mp4toannexb hevc_mp4toannexb aac_adtstoasc vp9_superframe_split extract_extradata)
ENCODERS=(h264_mf h264_nvenc h264_amf aac png pcm_s16le pcm_f32le)
MUXERS=(mp4 mov ipod wav image2)
FILTERS=(abuffer abuffersink buffer buffersink aresample aformat anull null format scale
         loudnorm ebur128 volume atrim trim asetpts setpts transpose hflip vflip)
HWACCELS=(h264_d3d11va h264_d3d11va2 hevc_d3d11va hevc_d3d11va2 vp9_d3d11va vp9_d3d11va2 av1_d3d11va av1_d3d11va2)

./configure \
  --prefix="$PREFIX" --toolchain=msvc --arch=x86_64 --target-os=win64 \
  --enable-shared --disable-static \
  --disable-gpl --disable-nonfree \
  --disable-autodetect --enable-w32threads \
  --enable-d3d11va --enable-mediafoundation \
  --enable-ffnvcodec --enable-nvenc --enable-amf --enable-libdav1d \
  --pkg-config=pkgconf --pkg-config-flags=--static \
  --disable-programs --disable-doc --disable-debug --disable-network --disable-avdevice \
  --disable-everything --enable-protocol=file,pipe \
  --enable-demuxer="$(join "${DEMUXERS[@]}")" \
  --enable-decoder="$(join "${DECODERS[@]}")" \
  --enable-parser="$(join "${PARSERS[@]}")" \
  --enable-bsf="$(join "${BSFS[@]}")" \
  --enable-encoder="$(join "${ENCODERS[@]}")" \
  --enable-muxer="$(join "${MUXERS[@]}")" \
  --enable-filter="$(join "${FILTERS[@]}")" \
  --enable-hwaccel="$(join "${HWACCELS[@]}")" \
  --enable-avcodec --enable-avformat --enable-avfilter --enable-swscale --enable-swresample \
  --extra-cflags="-MT -I$(cygpath -m "$DEPS/include")" --extra-ldflags="-LIBPATH:$(cygpath -m "$DEPS/lib")" \
  || { tail -40 ffbuild/config.log; exit 1; }

# --disable-autodetect drops every library not named: turn a silent loss into a failed build.
for flag in 'CONFIG_GPL 0' 'CONFIG_NONFREE 0' 'HAVE_W32THREADS 1' 'CONFIG_H264_MF_ENCODER 1' \
            'CONFIG_H264_NVENC_ENCODER 1' 'CONFIG_H264_AMF_ENCODER 1' 'CONFIG_LIBDAV1D 1' \
            'CONFIG_H264_D3D11VA_HWACCEL 1' 'CONFIG_AV1_D3D11VA_HWACCEL 1'; do
  # Since FFmpeg 6, per-component switches (encoders, hwaccels) live in config_components.h.
  cat config.h config_components.h 2>/dev/null | grep -q "^#define $flag\$" || {
    echo "configure: expected $flag" >&2
    echo "--- config.h (MF, MEDIAFOUNDATION, NVENC, AMF, D3D11)"; grep -E "MEDIAFOUNDATION|_MF_|NVENC|AMF|D3D11VA|LIBDAV1D" config.h config_components.h || true
    echo "--- config.log around mediafoundation"; grep -n -B2 -A14 -i "mediafoundation\|mfapi\|mftransform\|codecapi" ffbuild/config.log | tail -80 || true
    exit 1; }
done

make -j"$JOBS"
rm -rf "$PREFIX" && make install
cp "$SRC/COPYING.LGPLv2.1" "$PREFIX/LICENSE.txt"
echo "--- license"; grep -E '^#define CONFIG_(GPL|NONFREE|VERSION3) ' config.h
echo "--- DLLs"; ls -la "$PREFIX/bin"/*.dll
du -ch "$PREFIX/bin"/*.dll | tail -1
