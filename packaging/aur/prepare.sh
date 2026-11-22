#!/usr/bin/env bash
set -euo pipefail

deb=$1
output=$2
project_url=$3
deb_url=${4:-}
repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)

version=$(dpkg-deb -f "$deb" Version)
if [[ $(dpkg-deb -f "$deb" Architecture) != amd64 ||
      ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[A-Za-z0-9.-]+)?(\+[A-Za-z0-9.-]+)?$ ]]; then
  printf '无效的 amd64 .deb 架构或版本：%s\n' "$version" >&2
  exit 1
fi

# Pacman 的 pkgver 不能含连字符，预发行版应排在对应的正式版之前。
pkgver=${version%%+*}
if [[ "$pkgver" == *-* ]]; then
  pkgver="${pkgver%%-*}pre.${pkgver#*-}"
  pkgver=${pkgver//-/.}
fi

mkdir -p -- "$output"
deb_name="revo-panel-bin-${pkgver}.deb"
deb_source=$deb_name
if [[ -n "$deb_url" ]]; then
  deb_source+="::$deb_url"
else
  cp -- "$deb" "$output/$deb_name"
fi
deb_sha=$(sha256sum < "$deb")
deb_sha=${deb_sha%% *}

license=$(jq -r '.license // ""' "$repo/package.json")
spdx_pattern='^[A-Za-z0-9.+(): -]+$'
license_sha=
if [[ -n "$deb_url" || -n "$license" || -e "$repo/LICENSE" ]]; then
  if [[ ! -s "$repo/LICENSE" || ! "$license" =~ $spdx_pattern ]]; then
    printf '请添加项目 LICENSE，并在 package.json 中填写对应的 SPDX 许可证标识\n' >&2
    exit 1
  fi
  cp -- "$repo/LICENSE" "$output/LICENSE"
  license_sha=$(sha256sum < "$repo/LICENSE")
  license_sha=${license_sha%% *}
fi

{
  printf 'pkgname=revo-panel-bin\npkgver=%q\nurl=%q\n' "$pkgver" "$project_url"
  if [[ -n "$license_sha" ]]; then
    printf 'license=(%q)\nsource=(LICENSE)\nsha256sums=(%q)\n' "$license" "$license_sha"
  fi
  printf 'source_x86_64=(%q)\nsha256sums_x86_64=(%q)\nnoextract=(%q)\n' "$deb_source" "$deb_sha" "$deb_name"
  cat "$repo/packaging/aur/PKGBUILD.in"
} > "$output/PKGBUILD"
