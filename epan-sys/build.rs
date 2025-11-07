#[cfg(feature = "bindgen")]
extern crate bindgen;

use anyhow::Result;
use cargo_metadata::MetadataCommand;
use lazy_static::lazy_static;
use std::env;
use std::path::PathBuf;

#[cfg(target_os = "windows")]
use std::process::Command;

lazy_static! {
    static ref WIRESHARK_VERSION: String = {
        let metadata = MetadataCommand::new().exec().unwrap();
        metadata
            .workspace_metadata
            .get("wireshark_version")
            .and_then(|v| v.as_str())
            .expect("Wireshark version must be set in Cargo.toml under workspace.metadata")
            .to_string()
    };
    static ref WIRESHARK_SOURCE_DIR: PathBuf = {
        env::var("WIRESHARK_SOURCE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                #[cfg(target_os = "windows")]
                {
                    PathBuf::from(format!("C:\\wsbuild\\wireshark-{}", *WIRESHARK_VERSION))
                }
                #[cfg(not(target_os = "windows"))]
                {
                    PathBuf::from(format!("wireshark-{}", *WIRESHARK_VERSION))
                }
            })
    };
    static ref WIRESHARK_BUILD_DIR: PathBuf = {
        env::var("WIRESHARK_BUILD_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                #[cfg(target_os = "windows")]
                {
                    PathBuf::from("C:\\wsbuild\\build")
                }
                #[cfg(not(target_os = "windows"))]
                {
                    WIRESHARK_SOURCE_DIR.join("build")
                }
            })
    };
}

fn main() -> Result<()> {
    eprintln!("Wireshark source directory: {:?}", *WIRESHARK_SOURCE_DIR);
    eprintln!("Wireshark build directory: {:?}", *WIRESHARK_BUILD_DIR);

    if env::var("DOCS_RS").is_ok() {
        return Ok(());
    }

    #[cfg(feature = "bindgen")]
    generate_bindings()?;

    link_wireshark()?;
    Ok(())
}

fn link_wireshark() -> Result<()> {
    if pkg_config::probe_library("wireshark").is_ok() {
        return Ok(());
    }

    println!("cargo:rerun-if-env-changed=WIRESHARK_LIB_DIR");
    let build_from_source = if let Ok(libws_dir) = env::var("WIRESHARK_LIB_DIR") {
        if !libws_dir.is_empty() {
            println!("cargo:rustc-link-search=native={}", libws_dir);
            false
        } else {
            true
        }
    } else {
        true
    };

    if build_from_source {
        // Determine build configuration
        let build_config = if cfg!(debug_assertions) {
            "Debug"
        } else {
            "Release"
        };

        let lib_dir = WIRESHARK_BUILD_DIR.join(format!("run\\{}", build_config));

        // If library directory doesn't exist, build Wireshark
        if !lib_dir.exists() {
            eprintln!("Wireshark library directory not found, building...");
            download_wireshark(true)?;
            build_wireshark()?;
        }

        eprintln!("Found Wireshark libraries at: {:?}", lib_dir);
        println!("cargo:rustc-link-search=native={}", lib_dir.display());
    }

    // Link Wireshark and supporting libraries
    println!("cargo:rustc-link-lib=dylib=wireshark");
    println!("cargo:rustc-link-lib=dylib=wiretap");
    println!("cargo:rustc-link-lib=dylib=wsutil");

    Ok(())
}

fn download_wireshark(skip_existing: bool) -> Result<()> {
    if skip_existing && WIRESHARK_SOURCE_DIR.exists() {
        return Ok(());
    }

    if WIRESHARK_SOURCE_DIR.exists() {
        std::fs::remove_dir_all(&*WIRESHARK_SOURCE_DIR)?;
    }

    let url = format!(
        "https://1.eu.dl.wireshark.org/src/all-versions/wireshark-{}.tar.xz",
        *WIRESHARK_VERSION
    );
    eprintln!("Downloading {}", url);

    let response = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60 * 5))
        .build()?
        .get(url)
        .send()?;

    #[cfg(target_os = "windows")]
    {
        let base_dir = PathBuf::from("C:\\wsbuild");
        std::fs::create_dir_all(&base_dir)?;
        let archive_path = base_dir.join(format!("wireshark-{}.tar.xz", *WIRESHARK_VERSION));

        std::fs::write(&archive_path, response.bytes()?)?;

        eprintln!("Extracting .xz archive...");
        let tar_path = base_dir.join(format!("wireshark-{}.tar", *WIRESHARK_VERSION));

        Command::new("7z.exe")
            .args([
                "x",
                &archive_path.to_string_lossy(),
                &format!("-o{}", base_dir.display()),
                "-y",
            ])
            .status()?
            .success()
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("Failed to extract .xz archive"))?;

        eprintln!("Extracting .tar archive...");
        Command::new("7z.exe")
            .args([
                "x",
                &tar_path.to_string_lossy(),
                &format!("-o{}", base_dir.display()),
                "-y",
            ])
            .status()?
            .success()
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("Failed to extract .tar archive"))?;

        let _ = std::fs::remove_file(tar_path);
    }

    #[cfg(not(target_os = "windows"))]
    {
        use tar::Archive;
        use xz2::read::XzDecoder;

        let bytes = response.bytes()?.to_vec();
        let readable = XzDecoder::new(bytes.as_slice());
        let mut archive = Archive::new(readable);
        archive.unpack(".")?;
    }

    Ok(())
}

fn get_cmake_build_options() -> Vec<(&'static str, &'static str)> {
    vec![
        // Disable all executables
        ("BUILD_androiddump", "OFF"),
        ("BUILD_capinfos", "OFF"),
        ("BUILD_captype", "OFF"),
        ("BUILD_ciscodump", "OFF"),
        ("BUILD_corbaidl2wrs", "OFF"),
        ("BUILD_dcerpcidl2wrs", "OFF"),
        ("BUILD_dftest", "OFF"),
        ("BUILD_dpauxmon", "OFF"),
        ("BUILD_dumpcap", "OFF"),
        ("BUILD_editcap", "OFF"),
        ("BUILD_etwdump", "OFF"),
        ("BUILD_logray", "OFF"),
        ("BUILD_mergecap", "OFF"),
        ("BUILD_randpkt", "OFF"),
        ("BUILD_randpktdump", "OFF"),
        ("BUILD_rawshark", "OFF"),
        ("BUILD_reordercap", "OFF"),
        ("BUILD_sshdump", "OFF"),
        ("BUILD_text2pcap", "OFF"),
        ("BUILD_tfshark", "OFF"),
        ("BUILD_tshark", "OFF"),
        ("BUILD_wifidump", "OFF"),
        ("BUILD_wireshark", "OFF"),
        ("BUILD_xxx2deb", "OFF"),
        // Disable optional features
        ("ENABLE_KERBEROS", "OFF"),
        ("ENABLE_SBC", "OFF"),
        ("ENABLE_SPANDSP", "OFF"),
        ("ENABLE_BCG729", "OFF"),
        ("ENABLE_AMRNB", "OFF"),
        ("ENABLE_ILBC", "OFF"),
        ("ENABLE_LIBXML2", "OFF"),
        ("ENABLE_OPUS", "OFF"),
        ("ENABLE_SINSP", "OFF"),
    ]
}

fn build_wireshark() -> Result<()> {
    eprintln!("Building Wireshark...");

    // Determine build configuration based on Rust profile
    let build_config = if cfg!(debug_assertions) {
        "Debug"
    } else {
        "Release"
    };

    #[cfg(target_os = "windows")]
    {
        std::fs::create_dir_all(&*WIRESHARK_BUILD_DIR)?;
        env::set_var("WIRESHARK_BASE_DIR", "C:\\wsbuild");

        // Use short path for CMake download/extraction to avoid long path errors
        let download_dir = PathBuf::from("C:\\wsbuild\\dl");
        std::fs::create_dir_all(&download_dir)?;
        env::set_var("WIRESHARK_DOWNLOAD_DIR", &download_dir);

        let mut args = vec![
            WIRESHARK_SOURCE_DIR.to_string_lossy().to_string(),
            "-G".to_string(),
            "Visual Studio 17 2022".to_string(),
            "-A".to_string(),
            "x64".to_string(),
            // Use short path for artifact downloads to avoid long path issues
            format!("-DWIRESHARK_DOWNLOAD_DIR={}", download_dir.display()),
        ];

        for (key, value) in get_cmake_build_options() {
            args.push(format!("-D{}={}", key, value));
        }

        eprintln!("Configuring with CMake...");
        Command::new("cmake")
            .current_dir(&*WIRESHARK_BUILD_DIR)
            .args(&args)
            .status()?
            .success()
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("CMake configuration failed"))?;

        eprintln!("Building with CMake (config: {})...", build_config);
        Command::new("cmake")
            .current_dir(&*WIRESHARK_BUILD_DIR)
            .args([
                "--build",
                ".",
                "--config",
                build_config,
                "--target",
                "ALL_BUILD",
            ])
            .status()?
            .success()
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("CMake build failed"))?;
    }

    #[cfg(not(target_os = "windows"))]
    {
        let mut config = cmake::Config::new(&*WIRESHARK_SOURCE_DIR);

        config.define("CMAKE_BUILD_TYPE", build_config);

        for (key, value) in get_cmake_build_options() {
            config.define(key, value);
        }

        config
            .out_dir(&*WIRESHARK_SOURCE_DIR)
            .very_verbose(true)
            .build();
    }

    eprintln!("Wireshark build completed successfully");
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
            let build_config = if cfg!(debug_assertions) {
                "Debug"
            } else {
                "Release"
            };

            let lib_dir = WIRESHARK_BUILD_DIR.join("run").join(build_config);

            if !lib_dir.exists() {
                download_wireshark(true)?;
                build_wireshark()?;
            }

            builder = builder
                .clang_arg(format!(
                    "-I{}",
                    WIRESHARK_SOURCE_DIR.join("include").to_string_lossy()
                ))
                .clang_arg(format!("-I{}", WIRESHARK_SOURCE_DIR.to_string_lossy()));

            #[cfg(target_os = "windows")]
            {
                let version_parts: Vec<_> = WIRESHARK_VERSION.split('.').collect();
                let major_minor = format!(
                    "{}.{}",
                    version_parts[0],
                    version_parts.get(1).unwrap_or(&"0")
                );

                if env::var("PKG_CONFIG_PATH").is_err() {
                    if let Ok(entries) = std::fs::read_dir("C:\\wsbuild") {
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
                                if let Some(pkg_path) = path
                                    .join("vcpkg-export")
                                    .read_dir()
                                    .ok()
                                    .and_then(|mut dirs| dirs.next())
                                    .and_then(|e| e.ok())
                                    .map(|e| {
                                        e.path().join("installed\\x64-windows\\lib\\pkgconfig")
                                    })
                                {
                                    if pkg_path.exists() {
                                        env::set_var("PKG_CONFIG_PATH", pkg_path);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }

                let glib = pkg_config::Config::new().probe("glib-2.0")?;
                for path in glib.include_paths {
                    builder = builder.clang_arg(format!("-I{}", path.to_string_lossy()));
                }
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
