#!/bin/bash
set -e

PREFIX="${1:-$HOME/.local}"
BINDIR="$PREFIX/bin"
SCRIPTDIR="$(cd "$(dirname "$0")" && pwd)"

# Find the binary: local build → cargo install → PATH
if [ -f "$SCRIPTDIR/bin/slurm-gpu" ]; then
    BINARY="$SCRIPTDIR/bin/slurm-gpu"
elif [ -f "$SCRIPTDIR/target/release/slurm-gpu" ]; then
    BINARY="$SCRIPTDIR/target/release/slurm-gpu"
elif command -v slurm-gpu >/dev/null 2>&1; then
    BINARY="$(command -v slurm-gpu)"
else
    echo "Error: slurm-gpu binary not found." >&2
    echo "Build first with 'cargo build --release' or install with:" >&2
    echo "  cargo install --git git@gitlab.oit.duke.edu:wjs/rusty-slurmgpu.git" >&2
    exit 1
fi

mkdir -p "$BINDIR"

# Copy binary (skip if already in target dir)
if [ "$(realpath "$BINARY")" != "$(realpath "$BINDIR/slurm-gpu" 2>/dev/null)" ]; then
    cp "$BINARY" "$BINDIR/slurm-gpu"
    chmod +x "$BINDIR/slurm-gpu"
fi

# Create symlinks
for cmd in slurm-report slurm-usage slurm-stat slurm-show-tres slurm-tui; do
    ln -sf slurm-gpu "$BINDIR/$cmd"
done

# Generate shell completions
COMPDIR_BASH="$PREFIX/share/bash-completion/completions"
COMPDIR_ZSH="$PREFIX/share/zsh/site-functions"
COMPDIR_FISH="$PREFIX/share/fish/vendor_completions.d"
mkdir -p "$COMPDIR_BASH" "$COMPDIR_ZSH" "$COMPDIR_FISH"
"$BINDIR/slurm-gpu" completions bash > "$COMPDIR_BASH/slurm-gpu" 2>/dev/null && echo "Bash completions installed."
"$BINDIR/slurm-gpu" completions zsh  > "$COMPDIR_ZSH/_slurm-gpu" 2>/dev/null && echo "Zsh completions installed."
"$BINDIR/slurm-gpu" completions fish > "$COMPDIR_FISH/slurm-gpu.fish" 2>/dev/null && echo "Fish completions installed."

# Install man page
MANDIR="$PREFIX/share/man/man1"
if [ -f "$SCRIPTDIR/man/slurm-gpu.1" ]; then
    mkdir -p "$MANDIR"
    cp "$SCRIPTDIR/man/slurm-gpu.1" "$MANDIR/slurm-gpu.1"
    # Create man page symlinks for each command
    for cmd in slurm-report slurm-usage slurm-stat slurm-show-tres slurm-tui; do
        ln -sf slurm-gpu.1 "$MANDIR/$cmd.1"
    done
    echo "Man page installed to $MANDIR/"
fi

echo ""
echo "Installed to $BINDIR/"
echo "  slurm-gpu"
echo "  slurm-report  -> slurm-gpu report"
echo "  slurm-usage   -> slurm-gpu usage"
echo "  slurm-stat    -> slurm-gpu stat"
echo "  slurm-show-tres -> slurm-gpu show-tres"
echo "  slurm-tui     -> slurm-gpu tui"
echo ""
echo "Make sure $BINDIR is in your PATH."
echo "For man pages, ensure $MANDIR is in your MANPATH."
