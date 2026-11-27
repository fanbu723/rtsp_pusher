#!/usr/bin/env bash
# 容器内以 root 运行时切换到仓库所有者，避免 cargo 生成 root 所有的构建产物。
set -euo pipefail

if [ "$(id -u)" -ne 0 ]; then
    return 0 2>/dev/null || exit 0
fi

repo_root=${ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}
owner_uid=$(stat -c '%u' "$repo_root")
owner_gid=$(stat -c '%g' "$repo_root")

if [ "$owner_uid" -eq 0 ] || ! command -v setpriv >/dev/null 2>&1; then
    return 0 2>/dev/null || exit 0
fi

owner_home=$(getent passwd "$owner_uid" 2>/dev/null | cut -d: -f6 || true)
if [ -z "$owner_home" ]; then
    for candidate in /home/*; do
        if [ -d "$candidate" ] && [ "$(stat -c '%u' "$candidate")" = "$owner_uid" ]; then
            owner_home="$candidate"
            break
        fi
    done
fi
if [ -z "$owner_home" ]; then
    owner_home=/tmp
fi

echo "以仓库所有者 UID $owner_uid 执行，避免生成 root 所有文件" >&2
exec setpriv --reuid="$owner_uid" --regid="$owner_gid" --clear-groups \
    /usr/bin/env HOME="$owner_home" bash "$0" "$@"
