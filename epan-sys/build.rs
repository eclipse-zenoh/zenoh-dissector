#[cfg(feature = "bindgen")]
extern crate bindgen;

use anyhow::Result;
use cargo_metadata::MetadataCommand;
use lazy_static::lazy_static;
use std::env;
use std::path::PathBuf;

lazy_static! {
    static ref WIRESHARK_VERSION: String = {
        let metadata = MetadataCommand::new().exec().unwrap();
        metadata
            .workspace_metadata
            .get("wireshark_version")
            .and_then(|v| v.as_str())
            .expect("Wireshark version must be set in Cargo.toml under [workspace.metadata]")
            .to_string()
    };
    static ref WIRESHARK_SOURCE_DIR: PathBuf = PathBuf::from(format!(
        "{}/wireshark-{}",
        env::var("CARGO_MANIFEST_DIR").unwrap(),
        *WIRESHARK_VERSION
    ));
    static ref WIRESHARK_BUILD_DIR: PathBuf = WIRESHARK_SOURCE_DIR.join("build");
}

fn main() -> Result<()> {
    // If we are in docs.rs, there is no need to actually link.
    if std::env::var("DOCS_RS").is_ok() {
        return Ok(());
    }

    // Re-generate the bindings at build time.
    #[cfg(feature = "bindgen")]
    generate_bindings()?;

    link_wireshark()?;
    Ok(())
}

fn link_wireshark() -> Result<()> {
    // pkg-config will handle everything for us
    if pkg_config::probe_library("wireshark").is_ok() {
        return Ok(());
    }

    // Follow the given environmental variable WIRESHARK_LIB_DIR
    println!("cargo:rerun-if-env-changed=WIRESHARK_LIB_DIR");

    let build_from_source = {
        if let Ok(libws_dir) = env::var("WIRESHARK_LIB_DIR") {
            if libws_dir.is_empty() {
                true
            } else {
                println!("cargo:rustc-link-search=native={}", libws_dir);
                false
            }
        } else {
            true
        }
    };

    if build_from_source {
        // WARN: We can't build from source with cmake if WIRESHARK_LIB_DIR is set. That's why we
        // need to separate the branches.

        // WARN: eventually we don't use this directly due to the privilege issue.
        // instead, we will link the /applications/wireshark.app/contents/frameworks/libwireshark.*.dylib
        // to libwireshark.dylib under the project folder and configure it via WIRESHARK_LIB_DIR
        //
        // // Default wireshark libraray installed on macos
        // #[cfg(target_os = "macos")]
        // {
        //     let macos_wireshark_library = "/Applications/wireshark.app/contents/frameworks";
        //     if !PathBuf::from(macos_wireshark_library).exists() {
        //         panic!("wireshark library not found at {macos_wireshark_library}");
        //     }
        //     println!("cargo:rustc-link-search=native={macos_wireshark_library}");
        // }

        if !WIRESHARK_BUILD_DIR.exists() {
            download_wireshark(true)?;
            build_wireshark();
        }
        println!(
            "cargo:rustc-link-search=native={}",
            WIRESHARK_SOURCE_DIR.to_string_lossy()
        );
    }

    println!("cargo:rustc-link-lib=dylib=wireshark");

    Ok(())
}

#[cfg(feature = "bindgen")]
fn generate_bindings() -> Result<()> {
    let mut builder = bindgen::Builder::default()
        .header("wrapper.h")
        .generate_comments(false);

    match pkg_config::probe_library("wireshark") {
        Ok(libws) => {
            for path in libws.include_paths {
                builder = builder.clang_arg(format!("-I{}", path.to_string_lossy()));
            }
        }
        Err(_) => {
            download_wireshark(true)?;
            build_wireshark();

            // ws_version.h is under wireshark-{ver}/build
            builder = builder.clang_arg(format!("-I{}", WIRESHARK_BUILD_DIR.to_string_lossy()));

            // general header files are under wireshark-{ver} and wireshark-{ver}/include
            builder = builder.clang_arg(format!(
                "-I{}",
                WIRESHARK_SOURCE_DIR.join("include").to_string_lossy()
            ));
            builder = builder.clang_arg(format!("-I{}", WIRESHARK_SOURCE_DIR.to_string_lossy()));

            // glib-2.0 is installed under vcpkg
            #[cfg(target_os = "windows")]
            {
                // Extract major.minor version (e.g., "4.6.0" -> "4.6")
                let version_parts: Vec<_> = WIRESHARK_VERSION.split('.').collect();
                let major_minor = format!(
                    "{}.{}",
                    version_parts[0],
                    version_parts.get(1).unwrap_or(&"0")
                );

                // Try to find the vcpkg directory with glob pattern
                let base_path = format!("C:\\Development\\wireshark-x64-libs-{}", major_minor);

                // If PKG_CONFIG_PATH is not already set, try to find it
                if env::var("PKG_CONFIG_PATH").is_err() {
                    if let Ok(entries) = std::fs::read_dir("C:\\Development") {
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if path.is_dir()
                                && path
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .map(|s| {
                                        s.starts_with(&format!(
                                            "wireshark-x64-libs-{}",
                                            major_minor
                                        ))
                                    })
                                    .unwrap_or(false)
                            {
                                let pkg_config_path = path
                                    .join("vcpkg-export")
                                    .read_dir()
                                    .ok()
                                    .and_then(|mut dirs| dirs.next())
                                    .and_then(|e| e.ok())
                                    .map(|e| {
                                        e.path().join("installed\\x64-windows\\lib\\pkgconfig")
                                    });

                                if let Some(pkg_path) = pkg_config_path {
                                    if pkg_path.exists() {
                                        env::set_var("PKG_CONFIG_PATH", pkg_path);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // header files for glib-2.0
            let glib = pkg_config::Config::new().probe("glib-2.0")?;
            for path in glib.include_paths {
                builder = builder.clang_arg(format!("-I{}", path.to_string_lossy()));
            }
        }
    }

    let bindings = builder.generate()?;
    let out_path = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    #[cfg(not(target_os = "windows"))]
    bindings.write_to_file(out_path.join("bindings.rs"))?;
    #[cfg(target_os = "windows")]
    bindings.write_to_file(out_path.join("bindings_windows.rs"))?;

    Ok(())
}

fn download_wireshark(skip_existing: bool) -> Result<()> {
    if skip_existing && WIRESHARK_SOURCE_DIR.exists() {
        return Ok(());
    }

    if WIRESHARK_SOURCE_DIR.exists() {
        std::fs::remove_dir_all(&*WIRESHARK_SOURCE_DIR)?;
    }

    use tar::Archive;
    use xz2::read::XzDecoder;

    let url = format!(
        // "https://2.na.dl.wireshark.org/src/all-versions/wireshark-{}.tar.xz",
        "https://1.eu.dl.wireshark.org/src/all-versions/wireshark-{}.tar.xz",
        *WIRESHARK_VERSION
    );
    eprintln!("Downloading {url}");

    // Tackle the too-slow downloading problem
    let response = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60 * 5))
        .build()?
        .get(url)
        .send()?;
    let bytes = response.bytes()?.to_vec();
    let readable = XzDecoder::new(bytes.as_slice());
    let mut archive = Archive::new(readable);
    archive.unpack(".")?;

    Ok(())
}

fn build_wireshark() {
    #[cfg(target_os = "windows")]
    {
        // This installs the vcpkg under C:\\Development\wireshark-x64-libs-{ver}
        env::set_var("WIRESHARK_BASE_DIR", "C:\\Development");
        env::set_var("PLATFORM", "win64");
    }

    // The generated files will be directed to WIRESHARK_BUILD_DIR instead of the default OUT_DIR, enhancing reusability.
    let _ = cmake::Config::new(&*WIRESHARK_SOURCE_DIR)
        .define("BUILD_androiddump", "OFF")
        .define("BUILD_capinfos", "OFF")
        .define("BUILD_captype", "OFF")
        .define("BUILD_ciscodump", "OFF")
        .define("BUILD_corbaidl2wrs", "OFF")
        .define("BUILD_dcerpcidl2wrs", "OFF")
        .define("BUILD_dftest", "OFF")
        .define("BUILD_dpauxmon", "OFF")
        .define("BUILD_dumpcap", "OFF")
        .define("BUILD_editcap", "OFF")
        .define("BUILD_etwdump", "OFF")
        .define("BUILD_logray", "OFF")
        .define("BUILD_mergecap", "OFF")
        .define("BUILD_randpkt", "OFF")
        .define("BUILD_randpktdump", "OFF")
        .define("BUILD_rawshark", "OFF")
        .define("BUILD_reordercap", "OFF")
        .define("BUILD_sshdump", "OFF")
        .define("BUILD_text2pcap", "OFF")
        .define("BUILD_tfshark", "OFF")
        .define("BUILD_tshark", "OFF")
        .define("BUILD_wifidump", "OFF")
        .define("BUILD_wireshark", "OFF")
        .define("BUILD_xxx2deb", "OFF")
        .define("ENABLE_KERBEROS", "OFF")
        .define("ENABLE_SBC", "OFF")
        .define("ENABLE_SPANDSP", "OFF")
        .define("ENABLE_BCG729", "OFF")
        .define("ENABLE_AMRNB", "OFF")
        .define("ENABLE_ILBC", "OFF")
        .define("ENABLE_LIBXML2", "OFF")
        .define("ENABLE_OPUS", "OFF")
        .define("ENABLE_SINSP", "OFF")
        .out_dir(&*WIRESHARK_SOURCE_DIR)
        .very_verbose(true)
        .build();
}
