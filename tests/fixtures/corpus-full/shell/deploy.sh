#!/usr/bin/env bash
set -euo pipefail

APP_NAME="webapp"
DEPLOY_DIR="/opt/${APP_NAME}"
LOG_DIR="/var/log/${APP_NAME}"

function log() {
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*"
}

function check_deps() {
    local deps=("docker" "git" "curl")
    for dep in "${deps[@]}"; do
        if ! command -v "${dep}" &>/dev/null; then
            log "ERROR: missing dependency: ${dep}"
            exit 1
        fi
    done
}

function build_image() {
    local tag="${1:-latest}"
    log "Building Docker image: ${APP_NAME}:${tag}"
    docker build -t "${APP_NAME}:${tag}" .
}

function deploy() {
    local tag="${1:-latest}"
    log "Deploying ${APP_NAME}:${tag}"
    docker stop "${APP_NAME}" 2>/dev/null || true
    docker rm "${APP_NAME}" 2>/dev/null || true
    docker run -d \
        --name "${APP_NAME}" \
        --restart unless-stopped \
        -p 8000:8000 \
        "${APP_NAME}:${tag}"
}

function health_check() {
    local retries=5
    local url="http://localhost:8000/health"
    for i in $(seq 1 "${retries}"); do
        if curl -sf "${url}" &>/dev/null; then
            log "Health check passed"
            return 0
        fi
        log "Health check attempt ${i}/${retries} failed, retrying..."
        sleep 2
    done
    log "ERROR: health check failed after ${retries} attempts"
    exit 1
}

check_deps
build_image "${1:-latest}"
deploy "${1:-latest}"
health_check
