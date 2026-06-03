#!/usr/bin/env bash
set -euo pipefail

source ./deploy.sh

function setup_database() {
    log "Setting up database..."
    createdb webapp 2>/dev/null || log "Database already exists"
    psql webapp -f sql/schema.sql
}

function create_admin() {
    local username="${1:-admin}"
    local email="${2:-admin@example.com}"
    log "Creating admin user: ${username}"
}

function install_deps() {
    log "Installing system dependencies..."
    apt-get update -qq
    apt-get install -y --no-install-recommends \
        postgresql-client \
        curl \
        git
}

install_deps
setup_database
create_admin
log "Setup complete"
