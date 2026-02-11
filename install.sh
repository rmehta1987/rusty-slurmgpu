#!/bin/bash
set -e

PREFIX="${1:-$HOME/.local}"
BINDIR="$PREFIX/bin"

mkdir -p "$BINDIR"

# Copy binary
cp target/release/slurm-gpu "$BINDIR/slurm-gpu"
chmod +x "$BINDIR/slurm-gpu"

# Create symlinks
for cmd in slurm-report slurm-usage slurm-stat slurm-show-tres slurm-tui; do
    ln -sf slurm-gpu "$BINDIR/$cmd"
done

# Install man page
MANDIR="$PREFIX/share/man/man1"
SCRIPTDIR="$(cd "$(dirname "$0")" && pwd)"
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
