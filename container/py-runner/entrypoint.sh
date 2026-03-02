#!/bin/bash
# entrypoint.sh — create a user matching the host UID/GID so that files
# written to mounted volumes are owned by the correct host user.
# Then drop to that user and run the agent.
set -e

HOST_UID="${HOST_UID:-1000}"
HOST_GID="${HOST_GID:-1000}"
USERNAME="agent"

# Create group if it doesn't exist
if ! getent group "${HOST_GID}" > /dev/null 2>&1; then
    groupadd -g "${HOST_GID}" "${USERNAME}group"
fi

# Create user if it doesn't exist
if ! getent passwd "${HOST_UID}" > /dev/null 2>&1; then
    useradd --no-log-init -o -u "${HOST_UID}" -g "${HOST_GID}" \
            -m -d "/home/${USERNAME}" -s /bin/bash "${USERNAME}"
fi

UNAME=$(getent passwd "${HOST_UID}" | cut -d: -f1)

# Grant passwordless sudo so the agent can install system packages
echo "${UNAME} ALL=(ALL) NOPASSWD:ALL" > /etc/sudoers.d/agent-user
chmod 0440 /etc/sudoers.d/agent-user

# Set up persistent Python env directory
ENV_DIR="/workspace/env"
# Ubuntu pip (Debian-based) installs to local/lib/python3.12/dist-packages when using --prefix
# Include both paths to support both pip behaviors
mkdir -p \
    "${ENV_DIR}/local/lib/python3.12/dist-packages" \
    "${ENV_DIR}/lib/python3.12/site-packages" \
    "${ENV_DIR}/local/bin" \
    "${ENV_DIR}/bin"
chown -R "${HOST_UID}:${HOST_GID}" "${ENV_DIR}" 2>/dev/null || true
chown -R "${HOST_UID}:${HOST_GID}" /workspace 2>/dev/null || true

# Configure PYTHONPATH so packages installed to /workspace/env are importable
export PYTHONPATH="${ENV_DIR}/local/lib/python3.12/dist-packages:${ENV_DIR}/lib/python3.12/site-packages:${PYTHONPATH:-}"
export PATH="${ENV_DIR}/local/bin:${ENV_DIR}/bin:/usr/local/bin:${PATH}"
export HOME="/home/${USERNAME}"

# Drop privileges and run the agent (or custom command if provided)
if [ $# -gt 0 ]; then
    exec gosu "${HOST_UID}:${HOST_GID}" \
        env HOME="${HOME}" PYTHONPATH="${PYTHONPATH}" PATH="${PATH}" \
        "$@"
else
    exec gosu "${HOST_UID}:${HOST_GID}" \
        env HOME="${HOME}" PYTHONPATH="${PYTHONPATH}" PATH="${PATH}" \
        python3 /app/agent.py
fi
