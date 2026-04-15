#!/usr/bin/env nu

const wireshark_cask = "/Applications/Wireshark.app/Contents/Frameworks"

if not ($wireshark_cask | path exists) {
    error make -u {msg: "Wireshark Homebrew Cask not found"}
}

const target_dir = (path self | path dirname | path join ".." ".." "target" "setup")
mkdir $target_dir

# Create unversioned symlinks in target/ so the linker can find versioned dylibs
for lib in ["wireshark" "wiretap" "wsutil"] {
    let versioned = (glob ([$wireshark_cask $"lib($lib).*.dylib"] | path join) | first)
    let link = ([$target_dir $"lib($lib).dylib"] | path join)
    rm -f $link
    ln -s $versioned $link
}

# Needed by cargo-build
$env.WIRESHARK_LIB_DIR = $target_dir
# Needed by cargo-test
$env.DYLD_LIBRARY_PATH = $wireshark_cask
