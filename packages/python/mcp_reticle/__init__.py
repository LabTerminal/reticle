"""Reticle - Real-time debugging proxy for MCP servers."""

__version__ = "0.1.0"

import os
import sys
import platform
import subprocess
from pathlib import Path


def get_binary_path() -> Path:
    """Find the reticle binary for the current platform."""
    system = platform.system().lower()
    machine = platform.machine().lower()

    # Normalize architecture names
    if machine in ('x86_64', 'amd64'):
        arch = 'x86_64'
    elif machine in ('arm64', 'aarch64'):
        arch = 'aarch64'
    else:
        arch = machine

    # Binary name
    binary_name = 'reticle.exe' if system == 'windows' else 'reticle'

    # Check in package directory
    package_dir = Path(__file__).parent
    binary_path = package_dir / 'bin' / f'{system}-{arch}' / binary_name

    if binary_path.exists():
        return binary_path

    # Check in PATH
    from shutil import which
    system_binary = which('reticle')
    if system_binary:
        return Path(system_binary)

    raise FileNotFoundError(
        f"Reticle binary not found for {system}-{arch}. "
        f"Install from source: cargo install reticle"
    )


def main():
    """Entry point for the reticle CLI."""
    try:
        binary = get_binary_path()
    except FileNotFoundError as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)

    # Make sure binary is executable
    if not os.access(binary, os.X_OK):
        os.chmod(binary, 0o755)

    # Execute binary with all arguments
    result = subprocess.run([str(binary)] + sys.argv[1:])
    sys.exit(result.returncode)


if __name__ == "__main__":
    main()
