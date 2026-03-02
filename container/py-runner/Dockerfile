FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive \
    TZ=UTC \
    LANG=C.UTF-8

# ── Build args for mirror support (China-friendly) ───────────────────────────
ARG APT_MIRROR=""
ARG PIP_INDEX_URL=""

# ── Optional: switch to a faster APT mirror inside China ────────────────────
RUN if [ -n "$APT_MIRROR" ]; then \
        sed -i "s|http://archive.ubuntu.com|${APT_MIRROR}|g" /etc/apt/sources.list.d/ubuntu.sources 2>/dev/null || true; \
        sed -i "s|http://security.ubuntu.com|${APT_MIRROR}|g" /etc/apt/sources.list.d/ubuntu.sources 2>/dev/null || true; \
    fi

# ── System packages ──────────────────────────────────────────────────────────
RUN apt-get update && apt-get install -y --no-install-recommends \
    # Core tools
    curl wget git vim nano less jq unzip zip \
    # Build essentials
    build-essential pkg-config ca-certificates gnupg \
    # Python 3.12
    python3.12 python3.12-venv python3.12-dev python3-pip \
    # Dynamic UID helper and sudo
    gosu sudo \
    # Playwright system deps (Chromium will be installed by playwright)
    libglib2.0-0 libnss3 libatk1.0-0 libatk-bridge2.0-0 \
    libcups2 libdrm2 libxkbcommon0 libxcomposite1 libxdamage1 \
    libxfixes3 libxrandr2 libgbm1 libasound2t64 \
    && rm -rf /var/lib/apt/lists/*

# ── Node.js 20 LTS ───────────────────────────────────────────────────────────
RUN curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && npm install -g pnpm \
    && rm -rf /var/lib/apt/lists/*

# ── Python: uv + pip ─────────────────────────────────────────────────────────
RUN if [ -n "$PIP_INDEX_URL" ]; then \
        pip install --break-system-packages --index-url "$PIP_INDEX_URL" uv; \
    else \
        pip install --break-system-packages uv; \
    fi

# ── Python: openai + playwright (agent dependencies) ─────────────────────────
RUN if [ -n "$PIP_INDEX_URL" ]; then \
        pip install --break-system-packages --index-url "$PIP_INDEX_URL" openai playwright; \
    else \
        pip install --break-system-packages openai playwright; \
    fi \
    && playwright install chromium --with-deps

# ── Agent source ─────────────────────────────────────────────────────────────
WORKDIR /app
COPY agent.py tools.py browser.py ./

# ── Workspace layout (volumes will be mounted over these) ────────────────────
RUN mkdir -p \
    /workspace/group \
    /workspace/env \
    /workspace/global \
    /workspace/ipc/messages \
    /workspace/ipc/tasks \
    /workspace/ipc/input

# ── Entrypoint: creates host-UID user, grants sudo, then drops privileges ────
COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

ENTRYPOINT ["/entrypoint.sh"]
