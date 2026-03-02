#!/bin/bash
# Build the NanoClaw Python agent container.
#
# Usage:
#   ./build.sh               # normal build
#   ./build.sh --mirror      # use China mirrors for APT and PyPI
#   ./build.sh --no-cache    # force clean rebuild
set -e
cd "$(dirname "$0")"

MIRROR=0
NO_CACHE=""
for arg in "$@"; do
    case "$arg" in
        --mirror)   MIRROR=1 ;;
        --no-cache) NO_CACHE="--no-cache" ;;
    esac
done

BUILD_ARGS=""
if [ "$MIRROR" = "1" ]; then
    echo "[build] Using China mirrors for APT and PyPI"
    BUILD_ARGS="--build-arg APT_MIRROR=https://mirrors.aliyun.com"
    BUILD_ARGS="$BUILD_ARGS --build-arg PIP_INDEX_URL=https://pypi.tuna.tsinghua.edu.cn/simple/"
fi

docker build $NO_CACHE $BUILD_ARGS -t nanoclaw-py-agent:latest .
echo "✓ Built nanoclaw-py-agent:latest"
