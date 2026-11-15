#!/usr/bin/env bash
set -euo pipefail

output_dir="${1:?usage: prepare-runtime-linux.sh OUTPUT_DIR}"
ffmpeg_asset="ffmpeg-master-latest-linux64-lgpl.tar.xz"
ffmpeg_url="https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/${ffmpeg_asset}"
ffmpeg_checksums_url="https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/checksums.sha256"
yt_dlp_version="2026.10.26"
yt_dlp_asset="yt-dlp_linux"
yt_dlp_base="https://github.com/yt-dlp/yt-dlp/releases/download/${yt_dlp_version}"
deno_version="2.9.8"
deno_asset="deno-x86_64-unknown-linux-gnu.zip"
deno_base="https://github.com/denoland/deno/releases/download/v${deno_version}"
curl_version="8.23.0-1"
curl_asset="curl-linux-x86_64-dev-${curl_version}.tar.xz"
curl_url="https://github.com/stunnel/static-curl/releases/download/${curl_version}/${curl_asset}"
curl_binary_sha256="715f85aae6d9caeca203f7c1f8697b02193c53f8dd6112d54647f0777290f23e"

work_dir="$(mktemp -d)"
trap 'rm -rf "${work_dir}"' EXIT
mkdir -p "${output_dir}/bin" "${output_dir}/licenses"

verify_manifest_entry() {
  local manifest="$1"
  local asset="$2"
  local file="$3"
  local expected
  expected="$(awk -v name="${asset}" '$NF == name || $NF == "*" name { print $1; exit }' "${manifest}")"
  if [[ ! "${expected}" =~ ^[0-9a-fA-F]{64}$ ]]; then
    echo "checksum not found for ${asset}" >&2
    exit 1
  fi
  echo "${expected}  ${file}" | sha256sum --check
}

curl -fsSL "${ffmpeg_url}" -o "${work_dir}/${ffmpeg_asset}"
curl -fsSL "${ffmpeg_checksums_url}" -o "${work_dir}/ffmpeg-checksums.sha256"
verify_manifest_entry "${work_dir}/ffmpeg-checksums.sha256" "${ffmpeg_asset}" "${work_dir}/${ffmpeg_asset}"
mkdir "${work_dir}/ffmpeg"
tar -xJf "${work_dir}/${ffmpeg_asset}" -C "${work_dir}/ffmpeg"
install -m755 "$(find "${work_dir}/ffmpeg" -type f -path '*/bin/ffmpeg' -print -quit)" "${output_dir}/bin/ffmpeg"
install -m755 "$(find "${work_dir}/ffmpeg" -type f -path '*/bin/ffprobe' -print -quit)" "${output_dir}/bin/ffprobe"

curl -fsSL "${yt_dlp_base}/${yt_dlp_asset}" -o "${work_dir}/${yt_dlp_asset}"
curl -fsSL "${yt_dlp_base}/SHA2-256SUMS" -o "${work_dir}/yt-dlp-checksums.sha256"
verify_manifest_entry "${work_dir}/yt-dlp-checksums.sha256" "${yt_dlp_asset}" "${work_dir}/${yt_dlp_asset}"
install -m755 "${work_dir}/${yt_dlp_asset}" "${output_dir}/bin/yt-dlp"

curl -fsSL "${deno_base}/${deno_asset}" -o "${work_dir}/${deno_asset}"
curl -fsSL "${deno_base}/${deno_asset}.sha256sum" -o "${work_dir}/deno-checksums.sha256"
verify_manifest_entry "${work_dir}/deno-checksums.sha256" "${deno_asset}" "${work_dir}/${deno_asset}"
mkdir "${work_dir}/deno"
unzip -q "${work_dir}/${deno_asset}" -d "${work_dir}/deno"
install -m755 "${work_dir}/deno/deno" "${output_dir}/bin/deno"

curl -fsSL "${curl_url}" -o "${work_dir}/${curl_asset}"
mkdir "${work_dir}/curl"
tar -xJf "${work_dir}/${curl_asset}" -C "${work_dir}/curl"
curl_binary="$(find "${work_dir}/curl" -type f -path '*/bin/curl' -print -quit)"
echo "${curl_binary_sha256}  ${curl_binary}" | sha256sum --check
install -m755 "${curl_binary}" "${output_dir}/bin/curl"

mkdir -p "${output_dir}/licenses/ffmpeg" "${output_dir}/licenses/curl"
find "${work_dir}/ffmpeg" -maxdepth 3 -type f \( -iname 'copying*' -o -iname 'license*' \) -exec cp -n {} "${output_dir}/licenses/ffmpeg/" \; || true
curl_license_dir="$(find "${work_dir}/curl" -type d -path '*/licenses' -print -quit)"
cp -R "${curl_license_dir}/." "${output_dir}/licenses/curl/"
curl -fsSL "https://raw.githubusercontent.com/yt-dlp/yt-dlp/${yt_dlp_version}/LICENSE" -o "${output_dir}/licenses/yt-dlp-UNLICENSE.txt"
curl -fsSL "https://raw.githubusercontent.com/denoland/deno/v${deno_version}/LICENSE.md" -o "${output_dir}/licenses/deno-MIT.txt"

ffmpeg_version="$("${output_dir}/bin/ffmpeg" -version | sed -n '1s/^ffmpeg version //p')"
yt_dlp_actual="$("${output_dir}/bin/yt-dlp" --version)"
deno_actual="$("${output_dir}/bin/deno" --version | sed -n '1s/^deno //p')"
curl_actual="$("${output_dir}/bin/curl" --version | sed -n '1s/^curl //p' | cut -d' ' -f1)"
cat > "${output_dir}/versions.json" <<EOF
{
  "ffmpeg": "${ffmpeg_version}",
  "yt-dlp": "${yt_dlp_actual}",
  "deno": "${deno_actual}",
  "curl": "${curl_actual}"
}
EOF
