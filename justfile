# Build release artifacts, update flake.nix hash, commit, and tag
release TYPE:
    python3 scripts/release.py release "{{TYPE}}"
    just update-server


update-server:
    ssh homeserver "cd /home/mat/nix; nix flake update blaz; up"
