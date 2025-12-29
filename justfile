# Reticle - MCP Protocol Inspector
# https://github.com/labterminal/reticle

# Default recipe - show available commands
default:
    @just --list

# ============================================================================
# Development
# ============================================================================

# Start the development environment (Tauri + Vite)
dev:
    #!/usr/bin/env bash
    set -euo pipefail
    ROOT="{{justfile_directory()}}"

    cleanup() {
        echo "Cleaning up..."
        pkill -f "vite.*frontend" 2>/dev/null || true
        lsof -ti:1420 | xargs kill -9 2>/dev/null || true
    }
    trap cleanup EXIT INT TERM

    echo "Starting Reticle in development mode..."

    # Kill any existing processes
    pkill -f "reticle" 2>/dev/null || true
    cleanup
    sleep 1

    # Start Vite dev server in background
    echo "Starting Vite dev server..."
    cd "$ROOT/frontend" && npm run dev &
    VITE_PID=$!

    # Wait for Vite to be ready
    echo "Waiting for Vite on http://localhost:1420..."
    for i in {1..30}; do
        if curl -s http://localhost:1420 > /dev/null 2>&1; then
            break
        fi
        sleep 1
    done

    # Start Tauri
    echo "Starting Tauri app..."
    cd "$ROOT/src-tauri" && cargo tauri dev

# Build for production
build:
    cd frontend && npm run build && cd ../src-tauri && cargo tauri build

# Check Rust code without building
check:
    cd crates/reticle-core && cargo check
    cd crates/reticle-cli && cargo check
    cd src-tauri && cargo check

# Run clippy lints
lint:
    cd crates/reticle-core && cargo clippy -- -D warnings
    cd crates/reticle-cli && cargo clippy -- -D warnings
    cd src-tauri && cargo clippy -- -D warnings

# Format code
fmt:
    cd crates/reticle-core && cargo fmt
    cd crates/reticle-cli && cargo fmt
    cd src-tauri && cargo fmt

# Run tests
test:
    cd crates/reticle-core && cargo test
    cd src-tauri && cargo test

# Build CLI only (to project target/release)
build-cli:
    CARGO_TARGET_DIR={{justfile_directory()}}/target cargo build --release --manifest-path crates/reticle-cli/Cargo.toml

# Build GUI app only
build-gui:
    cd src-tauri && cargo build --release

# Install CLI to ~/.local/bin
install-cli: build-cli
    mkdir -p ~/.local/bin
    cp target/release/reticle ~/.local/bin/
    @echo "Installed reticle to ~/.local/bin/reticle"

# Run GUI via CLI (uses reticle ui --dev if available)
ui:
    #!/usr/bin/env bash
    if command -v reticle &> /dev/null; then
        export RETICLE_PROJECT_ROOT="{{justfile_directory()}}"
        exec reticle ui --dev
    else
        echo "reticle CLI not found. Run 'just dev' instead or 'just install-cli' first."
        exit 1
    fi

# Install frontend dependencies
setup:
    cd frontend && npm install

# ============================================================================
# Testing - Direct (No Proxy)
# ============================================================================

# Test MCP server directly without proxy
test-direct:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Testing MCP server directly (no proxy)..."
    echo '{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}},"id":1}' | \
        npx -y @anthropics/mcp-server-memory 2>/dev/null | head -1

# ============================================================================
# Testing - With Proxy (reticle-cli)
# ============================================================================

# Test with proxy on default port (3001)
test-proxy port="3001":
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Testing MCP server through proxy on port {{port}}..."
    echo '{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}},"id":1}' | \
        ./target/release/reticle --port {{port}} -- npx -y @anthropics/mcp-server-memory 2>/dev/null | head -1

# Stress test with multiple sequential requests
test-stress count="100":
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Running stress test with {{count}} requests..."
    for i in $(seq 1 {{count}}); do
        echo '{"jsonrpc":"2.0","method":"ping","id":'$i'}' | \
            ./target/release/reticle --port 3001 -- cat 2>/dev/null
    done
    echo "Stress test complete: {{count}} requests sent"

# ============================================================================
# SSE Transport Testing
# ============================================================================

# Start mock SSE server (requires Python)
sse-server port="8080":
    python3 scripts/mock-mcp-sse-server.py --port {{port}}

# Test SSE endpoint
test-sse port="8080":
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Testing SSE endpoint on port {{port}}..."
    curl -s -N "http://localhost:{{port}}/events" &
    SSE_PID=$!
    sleep 1
    curl -s -X POST "http://localhost:{{port}}/message" \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","method":"initialize","params":{},"id":1}'
    sleep 1
    kill $SSE_PID 2>/dev/null || true

# ============================================================================
# Mock Servers (for development/testing)
# ============================================================================

# Run mock MCP server (stdio)
mock-server:
    python3 scripts/mock-mcp-server.py

# Run mock MCP agent
mock-agent:
    python3 scripts/mock-mcp-agent.py

# Run error test server
mock-error-server:
    python3 scripts/test-error-server.py

# ============================================================================
# Utilities
# ============================================================================

# Clean build artifacts
clean:
    rm -rf target/
    rm -rf frontend/node_modules/
    rm -rf frontend/dist/
    rm -rf src-tauri/target/
    rm -rf crates/reticle-core/target/
    rm -rf crates/reticle-cli/target/

# Show project info
info:
    @echo "Reticle - MCP Protocol Inspector"
    @echo "================================="
    @echo ""
    @echo "Project structure:"
    @echo "  frontend/           - React frontend"
    @echo "  src-tauri/          - Tauri desktop app (reticle-app)"
    @echo "  crates/reticle-core - Core library (protocol, token counting)"
    @echo "  crates/reticle-cli  - Standalone CLI proxy (reticle)"
    @echo "  scripts/            - Python test utilities"
    @echo ""
    @echo "Quick start:"
    @echo "  just setup       - Install dependencies"
    @echo "  just dev         - Start development server (Vite + Tauri)"
    @echo "  just ui          - Run GUI via CLI (requires: just install-cli)"
    @echo "  just build       - Build for production"
    @echo "  just build-cli   - Build CLI only"
    @echo "  just build-gui   - Build GUI app only"
    @echo "  just install-cli - Install CLI to ~/.local/bin"
    @echo "  just test        - Run all tests"

# Show current Rust/Node versions
versions:
    @echo "Rust: $(rustc --version)"
    @echo "Cargo: $(cargo --version)"
    @echo "Node: $(node --version)"
    @echo "npm: $(npm --version)"
