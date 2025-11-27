#!/usr/bin/env bash
set -euo pipefail

HOOK_DIR=".git/hooks"
HOOK_FILE="$HOOK_DIR/pre-push"

mkdir -p "$HOOK_DIR"

cat > "$HOOK_FILE" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

echo "Running pre-push checks..."

echo "1. cargo fmt --check"
cargo fmt --all -- --check

echo "2. cargo clippy"
cargo clippy --all-targets --all-features -- -D warnings

echo "3. cargo test"
cargo test --all-features --all-targets

echo "All checks passed."
EOF

chmod +x "$HOOK_FILE"

echo "Installed pre-push hook into $HOOK_FILE"
