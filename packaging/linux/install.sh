#!/bin/sh
# Installs Roch StressTest for the current user, without root: the program and the tool folders
# beside it to ~/.local/opt/RochStressTest (it looks for the tools beside itself), a
# `rochstresstest` command in ~/.local/bin, and the launcher entry and its icon.
#
# Run from the unpacked release tarball. Folders already installed are replaced, except y-cruncher
# profiles saved since (*.sh), which are kept.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
dest="$HOME/.local/opt/RochStressTest"
data=${XDG_DATA_HOME:-"$HOME/.local/share"}
[ -x "$here/RochStressTest" ] || { echo "Run this from the unpacked release folder." >&2; exit 1; }

# Profiles saved in the installed y-cruncher folder survive the reinstall.
keep=$(mktemp -d)
trap 'rm -rf "$keep"' EXIT
for profile in "$dest"/y-cruncher*/*.sh; do
    if [ -f "$profile" ]; then cp -p "$profile" "$keep/"; fi
done

mkdir -p "$dest"
for item in RochStressTest p95v3019b20.linux64 "y-cruncher v0.8.7.9547-static" memtest_vulkan README.md LICENSE; do
    rm -rf "${dest:?}/$item"
    cp -a "$here/$item" "$dest/"
done
for profile in "$keep"/*.sh; do
    if [ -f "$profile" ]; then cp -p "$profile" "$dest/y-cruncher v0.8.7.9547-static/"; fi
done

mkdir -p "$HOME/.local/bin"
printf '#!/bin/sh\nexec "%s/RochStressTest" "$@"\n' "$dest" > "$HOME/.local/bin/rochstresstest"
chmod 755 "$HOME/.local/bin/rochstresstest"
sed "s|^Exec=.*|Exec=\"$HOME/.local/bin/rochstresstest\"|" "$here/RochStressTest.desktop" > "$here/.desktop.tmp"
install -Dm644 "$here/.desktop.tmp" "$data/applications/RochStressTest.desktop"
rm -f "$here/.desktop.tmp"
install -Dm644 "$here/RochStressTest.png" "$data/icons/hicolor/256x256/apps/RochStressTest.png"
command -v update-desktop-database >/dev/null && update-desktop-database "$data/applications" || true
echo "Installed: $dest (command: rochstresstest, launcher: Roch StressTest)"
echo "OCCT is not included: download OCCT for Linux from https://www.ocbase.com/download and put it in $dest/OCCT/"
