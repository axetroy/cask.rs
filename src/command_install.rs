#![deny(warnings)]

extern crate flate2;
extern crate tar;

use crate::formula;
use crate::git;
use crate::util;
use crate::util::iso8601;

use std::env;
use std::fs;
use std::fs::{set_permissions, File};
use std::io;
use std::io::BufReader;
use std::io::Read;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use eyre::Report;
use flate2::read::GzDecoder;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tar::Archive;
use tinytemplate::TinyTemplate;

#[derive(Serialize)]
struct URLTemplateContext {
    name: String,
    bin: String,
    version: String,
}

pub async fn install(package_name: &str, version: Option<&str>) -> Result<(), Report> {
    let cask_git_url = format!("https://{}-cask.git", package_name);

    let unix_time = {
        let start = SystemTime::now();

        let t = start.duration_since(UNIX_EPOCH)?;

        t.as_secs()
    };

    let formula_cloned_dir = env::temp_dir().join(format!("cask_{}", unix_time));
    let cask_file_path = formula_cloned_dir.join("Cask.toml");

    let package_formula = match git::clone(&cask_git_url, &formula_cloned_dir, vec!["--depth", "1"])
    {
        Ok(()) => {
            if !cask_file_path.exists() {
                // remove cloned repo
                fs::remove_dir_all(formula_cloned_dir)?;
                return Err(eyre::format_err!(
                    "{} is not a valid formula!",
                    package_name
                ));
            }

            let f = formula::new(&cask_file_path)?;

            Ok(f)
        }
        Err(e) => Err(e),
    }?;

    let option_target = if cfg!(target_os = "macos") {
        package_formula.darwin.as_ref()
    } else if cfg!(target_os = "windows") {
        package_formula.windows.as_ref()
    } else if cfg!(target_os = "linux") {
        package_formula.linux.as_ref()
    } else {
        fs::remove_dir_all(formula_cloned_dir)?;
        return Err(eyre::format_err!(
            "{} not support your system",
            package_name
        ));
    };

    let target = match option_target {
        Some(p) => Ok(p),
        None => Err(eyre::format_err!(
            "{} not support your system",
            package_name
        )),
    }?;

    let hash_of_package = {
        let mut hasher = Sha256::new();

        hasher.update(package_name);
        format!("{:x}", hasher.finalize())
    };

    let home_dir = match dirs::home_dir() {
        Some(p) => Ok(p),
        None => Err(eyre::format_err!("can not get $HOME dir")),
    }?;

    let cask_dir = home_dir.join(".cask");
    let cask_dir_bin = cask_dir.join("bin");
    let cask_dir_formula = cask_dir.join("formula");
    let package_dir = cask_dir_formula.join(hash_of_package);

    // init formula folder
    {
        if !&cask_dir_bin.exists() {
            fs::create_dir_all(&cask_dir_bin)?;
        }

        if !&cask_dir_formula.exists() {
            fs::create_dir_all(&cask_dir_formula)?;
        }

        if !&package_dir.exists() {
            fs::create_dir_all(&package_dir)?;
            fs::create_dir_all(&package_dir.join("bin"))?;
            fs::create_dir_all(&package_dir.join("version"))?;
        }

        let cask_file_content = {
            let cask_file = File::open(&cask_file_path)?;
            let mut buf_reader = BufReader::new(&cask_file);
            let mut file_content = String::new();
            buf_reader.read_to_string(&mut file_content)?;

            file_content
        };

        let file_path = &package_dir.join("Cask.toml");

        let mut formula_file = File::create(&file_path)?;

        formula_file.write_all(
            format!(
                r#"# The file is generated by Cask. DO NOT MODIFY IT.
[cask]
package_name = "{}"
created_at = "{}"

"#,
                package_name,
                iso8601(&SystemTime::now())
            )
            .as_str()
            .as_bytes(),
        )?;
        formula_file.write_all(cask_file_content.as_bytes())?;
    }

    // remove cloned repo
    fs::remove_dir_all(formula_cloned_dir)?;

    let option_arch = if cfg!(target_arch = "x86") {
        target.x86.as_ref()
    } else if cfg!(target_arch = "x86_64") {
        target.x86_64.as_ref()
    } else if cfg!(target_arch = "arm") {
        target.arm.as_ref()
    } else if cfg!(target_arch = "aarch64") {
        target.aarch64.as_ref()
    } else if cfg!(target_arch = "mips") {
        target.mips.as_ref()
    } else if cfg!(target_arch = "mips64") {
        target.mips64.as_ref()
    } else if cfg!(target_arch = "mips64el") {
        target.mips64el.as_ref()
    } else {
        None
    };

    let arch = match option_arch {
        Some(a) => Ok(a),
        None => Err(eyre::format_err!("{} not support your arch", package_name)),
    }?;

    let download_version = {
        if let Some(v) = version {
            if !package_formula.package.versions.contains(&v.to_string()) {
                Err(eyre::format_err!(
                    "can not found version '{}' of formula",
                    v
                ))
            } else {
                Ok(v.to_owned())
            }
        } else if let Some(v) = &package_formula.package.version {
            if !package_formula.package.versions.contains(v) {
                Err(eyre::format_err!(
                    "can not found version '{}' of formula",
                    v
                ))
            } else {
                Ok(v.clone())
            }
        } else if package_formula.package.versions.is_empty() {
            Err(eyre::format_err!("can not found any version of formula"))
        } else {
            Ok(package_formula.package.versions[0].clone())
        }
    }?;

    let tar_file_path = &package_dir
        .join("version")
        .join(format!("{}.tar.gz", &download_version));
    let tar_file_name = tar_file_path.file_name().unwrap().to_str().unwrap();

    // renderer url
    let rendered_url = {
        let render_context = URLTemplateContext {
            name: package_formula.package.name.clone(),
            bin: package_formula.package.bin.clone(),
            version: download_version.clone(),
        };
        let mut tt = TinyTemplate::new();
        tt.add_template("url_template", &arch.url)?;

        tt.render("url_template", &render_context)?
    };

    util::download(&rendered_url, tar_file_path).await?;

    let tar_file = File::open(tar_file_path)?;

    let bin_name = if cfg!(target_os = "windows") {
        format!("{}.exe", &package_formula.package.bin)
    } else {
        package_formula.package.bin.clone()
    };

    let mut bin_found = false;

    let output_file_path = package_dir.join("bin").join(&bin_name);

    // .tar.gz
    if tar_file_name.ends_with(".tar.gz") {
        let tar = GzDecoder::new(&tar_file);
        let mut archive = Archive::new(tar);

        let files = archive.entries()?;

        for e in files {
            let mut entry = e?;

            let entry_file = entry.path()?;

            if let Some(file_name) = entry_file.file_name() {
                if file_name.to_str().unwrap() == bin_name {
                    entry.unpack(&output_file_path)?;
                    bin_found = true;
                    break;
                }
            }
        }
    } else if tar_file_name.ends_with(".zip") {
        let mut archive = zip::ZipArchive::new(&tar_file)?;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;

            if file.is_file() && file.name() == bin_name {
                let mut output_file = File::create(&output_file_path)?;

                io::copy(&mut file, &mut output_file)?;

                bin_found = true;

                // Get and Set permissions
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;

                    if let Some(mode) = file.unix_mode() {
                        set_permissions(&output_file_path, fs::Permissions::from_mode(mode))?;
                    }
                }
                break;
            }
        }
    }

    if !bin_found {
        return Err(eyre::format_err!(
            "can not found binary file '{}' in tar",
            bin_name
        ));
    } else {
        // create soft link in bin folder
        #[cfg(target_family = "unix")]
        std::os::unix::fs::symlink(output_file_path, cask_dir_bin.join(bin_name))?;
        #[cfg(target_family = "windows")]
        std::os::windows::fs::symlink_file(output_file_path, cask_dir_bin.join(bin_name))?;
    }

    Ok(())
}
