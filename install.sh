#!/bin/bash
set -e

echo "Installing clovis..."
cargo install --path .

echo "Generating shell completions..."

# Install bash completions
COMP_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/bash-completion/completions"
mkdir -p "$COMP_DIR"
clovis completions --generate bash > "$COMP_DIR/clovis"
echo "Bash completions installed to $COMP_DIR/clovis"

# Install zsh completions (system-wide, requires sudo)
if command -v zsh &> /dev/null; then
    echo "Installing zsh completions (requires sudo)..."
    sudo mkdir -p /usr/local/share/zsh/site-functions
    clovis completions --generate zsh | sudo tee /usr/local/share/zsh/site-functions/_clovis > /dev/null
    echo "Zsh completions installed to /usr/local/share/zsh/site-functions/_clovis"
fi

# Install fish completions
if command -v fish &> /dev/null; then
    COMP_DIR="$HOME/.config/fish/completions"
    mkdir -p "$COMP_DIR"
    clovis completions --generate fish > "$COMP_DIR/clovis.fish"
    echo "Fish completions installed to $COMP_DIR/clovis.fish"
fi

echo "Installation complete!"
echo "You may need to restart your shell for completions to take effect."
