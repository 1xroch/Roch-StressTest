#!/bin/sh
# Downloads the Linux builds of the tools Roch StressTest runs on Linux -- Prime95 (mprime),
# y-cruncher and memtest_vulkan -- from their authors' own sites, checks each against the
# SHA-256 below, and unpacks them beside the program, where it looks for them.
#
#   packaging/linux/fetch-tools.sh [DIR]     DIR defaults to the repository root
#
# They are not committed: y-cruncher's Linux build alone is 168 MB unpacked. The release tarball
# already contains them. OCCT is not fetched (it is someone else's to hand out); download OCCT for
# Linux from https://www.ocbase.com/download and put it in an OCCT folder beside the program.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
dest=$(mkdir -p "${1:-$repo}" && cd "${1:-$repo}" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

fetch() { # url sha256 file
    echo "Downloading $3"
    curl -fsSL --retry 3 -o "$work/$3" "$1"
    echo "$2  $work/$3" | sha256sum -c --quiet - || { echo "Checksum mismatch for $3" >&2; exit 1; }
}

if [ ! -x "$dest/p95v3019b20.linux64/mprime" ]; then
    fetch "https://www.mersenne.org/download/software/v30/30.19/p95v3019b20.linux64.tar.gz" \
        4ce2377e03deb4cf189523136e26401ba08f67857a128e420dd030d00cdca601 p95.tar.gz
    mkdir -p "$dest/p95v3019b20.linux64"
    tar -xzf "$work/p95.tar.gz" -C "$dest/p95v3019b20.linux64"
fi

yc="$dest/y-cruncher v0.8.7.9547-static"
if [ ! -x "$yc/y-cruncher" ]; then
    fetch "https://cdn.numberworld.org/y-cruncher-downloads/y-cruncher%20v0.8.7.9547-static.tar.xz" \
        e22ff344f42763520d2331f19ed6177f3a7c78ac5bd462e9c060c38267181490 y-cruncher.tar.xz
    tar -xJf "$work/y-cruncher.tar.xz" -C "$dest"
fi

if [ ! -x "$dest/memtest_vulkan/memtest_vulkan" ]; then
    fetch "https://github.com/GpuZelenograd/memtest_vulkan/releases/download/v0.5.0/memtest_vulkan-v0.5.0_DesktopLinux_X86_64.tar.xz" \
        0e058c28b9d0d6eb04f5edeebce73e8106f88c4ea307d46e98c715da61117d52 memtest_vulkan.tar.xz
    mkdir -p "$dest/memtest_vulkan"
    tar -xJf "$work/memtest_vulkan.tar.xz" -C "$dest/memtest_vulkan"
    chmod +x "$dest/memtest_vulkan/memtest_vulkan"
fi

# The y-cruncher profiles that ship for Windows, as the shell scripts the Linux tab reads: the
# same arguments, run from the folder they are in. One already there is left alone.
for bat in "$repo/y-cruncher v0.8.7.9547b"/*.bat; do
    [ -f "$bat" ] || continue
    name=$(basename "$bat" .bat)
    out="$yc/$name.sh"
    if [ -e "$out" ]; then continue; fi
    args=$(tr -d '\r' < "$bat" | sed -n 's/^.*y-cruncher\.exe[" ]*//p' | head -n 1)
    [ -n "$args" ] || continue
    printf '#!/bin/sh\n# A y-cruncher profile saved by Roch StressTest.\ncd "$(dirname "$0")" && exec ./y-cruncher %s\n' "$args" > "$out"
    chmod 755 "$out"
done

echo "Linux tools are in $dest"
