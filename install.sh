#!/bin/sh
set -eu
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
bin_dir="${HOME}/.local/bin"
skill_dir="${HOME}/.agents/skills"
with_skill=true
while [ "$#" -gt 0 ]; do
    case "$1" in
        --bin-dir|--skill-dir)
            option=$1
            [ "$#" -gt 1 ] && [ -n "$2" ] || { echo "$option requires a directory" >&2; exit 1; }
            case "$option" in --bin-dir) bin_dir=$2 ;; --skill-dir) skill_dir=$2 ;; esac
            shift 2 ;;
        --without-skill) with_skill=false; shift ;;
        --help|-h)
            echo 'Usage: ./install.sh [--bin-dir DIRECTORY] [--skill-dir DIRECTORY] [--without-skill]'
            echo 'Build and install patchwalk to ~/.local/bin and its Codex skill to ~/.agents/skills.'
            echo 'Rust is needed only to build. No shell configuration is modified.'
            exit 0 ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
done
command -v cargo >/dev/null 2>&1 || { echo 'Rust is required to build. Install it from https://rustup.rs and retry.' >&2; exit 1; }
# Pin the target directory so the copied executable is always the one just built.
cargo build --quiet --locked --release --manifest-path "$project_dir/Cargo.toml" --target-dir "$project_dir/target"
mkdir -p "$bin_dir"
staged_binary=$(mktemp "$bin_dir/.patchwalk.XXXXXX")
trap 'rm -f "$staged_binary"' EXIT HUP INT TERM
cp "$project_dir/target/release/patchwalk" "$staged_binary"
chmod 755 "$staged_binary"
mv -f "$staged_binary" "$bin_dir/patchwalk"
echo "Installed binary: $bin_dir/patchwalk"
if [ "$with_skill" = true ]; then
    mkdir -p "$skill_dir/patchwalk"
    cp "$project_dir/skills/patchwalk/SKILL.md" "$skill_dir/patchwalk/SKILL.md"
    echo "Installed agent skill: $skill_dir/patchwalk/SKILL.md"
fi
case ":$PATH:" in
    *":$bin_dir:"*) echo 'patchwalk is on PATH.' ;;
    *) printf 'Add this directory to your shell PATH: %s\n' "$bin_dir" ;;
esac
echo 'Try: patchwalk --demo'
echo 'In a new Codex chat, ask: Use $patchwalk to prepare this change for review.'
