#![deny(warnings)]

use crate::{cask, hooker};
use std::collections::HashMap;

use std::{
    env, fs,
    fs::File,
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use eyre::Report;
use serde::{Deserialize, Serialize};
use tinytemplate::TinyTemplate;
use url::Url;

#[derive(Deserialize, Serialize, Debug)]
pub struct Formula {
    #[serde(skip)]
    pub file_content: String, // The file content of this formula
    #[serde(skip)]
    pub repository: String, // The repository of this formula
    #[serde(skip)]
    pub filepath: PathBuf, // The filepath of this formula

    pub cask: Option<Cask>, // The cask information that generated by cask. This field is only available after the package is installed.
    pub package: Package,   // The package information
    pub context: Option<HashMap<String, String>>, // The hash map for renderer template
    pub windows: Option<Platform>, // The windows target information
    pub darwin: Option<Platform>, // The macOS target information
    pub linux: Option<Platform>, // The linux target information
    pub dependencies: Option<HashMap<String, Dependencies>>, // TODO: The dependencies of the package

    // The hooks defined
    pub hook: Option<hooker::Hook>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct Cask {
    pub name: String,       // The package name. eg github.com/axetroy/gpm.rs
    pub created_at: String, // The package installed date
    pub version: String,    // The version is using for package
    pub repository: String, // The package installed from the repository url
}

#[derive(Deserialize, Serialize, Debug)]
pub enum Dependencies {
    Detail(DependenciesDetail), // More information of the package
    Simple(String),             // The version of the package
}

#[derive(Deserialize, Serialize, Debug)]
pub struct DependenciesDetail {
    pub version: String, // The version of the package
}

#[derive(Deserialize, Serialize, Debug)]
pub struct Package {
    pub name: String,                  // The package name
    pub bin: String,                   // The binary name of the package
    pub repository: String,            // The repository url
    pub description: String,           // The description of the package
    pub versions: Option<Vec<String>>, // The version of package. If versions are not provide, cask will automatically get the versions from the repository tags.
    pub authors: Option<Vec<String>>,  // The author of package
    pub keywords: Option<Vec<String>>, // The keywords of the package
    pub license: Option<String>,       // The license of the package
}

#[derive(Deserialize, Serialize, Debug)]
pub struct Platform {
    pub x86: Option<ResourceTarget>,
    pub x86_64: Option<ResourceTarget>,
    pub arm: Option<ResourceTarget>,
    pub armv7: Option<ResourceTarget>,
    pub aarch64: Option<ResourceTarget>,
    pub mips: Option<ResourceTarget>,
    pub mips64: Option<ResourceTarget>,
    pub mips64el: Option<ResourceTarget>,
    pub riscv64: Option<ResourceTarget>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
pub enum ResourceTarget {
    Detailed(ResourceTargetDetail),
    Executable(ResourceTargetExecutable),
    Simple(String),
}

#[derive(Deserialize, Serialize, Debug)]
pub struct ResourceTargetDetail {
    pub url: String,              // The url will be download when install the package
    pub checksum: Option<String>, // The hash256 of download resource
    pub extension: Option<extractor::Extension>, // The extension name of download resource. optional value: ".tar.gz" ".tar" ".zip"
    pub path: Option<String>, // The folder that binary file locate in the tarball
}

#[derive(Deserialize, Serialize, Debug)]
pub struct ResourceTargetExecutable {
    pub executable: String, // The url will be download when install the package
    pub checksum: Option<String>, // The hash256 of download resource
}

pub fn new(formula_file: &Path, repo: &str) -> Result<Formula, Report> {
    let mut file = match File::open(formula_file) {
        Ok(f) => f,
        Err(e) => match e.kind() {
            ErrorKind::NotFound => Err(eyre::format_err!("the formula does not exist")),
            other_error => Err(eyre::format_err!("{:?}", other_error)),
        }?,
    };

    let mut file_content = String::new();

    match file.read_to_string(&mut file_content) {
        Ok(_) => {}
        Err(e) => {
            return Err(eyre::Report::from(e));
        }
    };

    drop(file);

    let mut f: Formula = match toml::from_str(&file_content) {
        Ok(r) => r,
        Err(e) => return Err(eyre::Report::from(e)),
    };

    f.filepath = formula_file.to_path_buf();
    f.repository = repo.to_string();
    f.file_content = file_content;

    Ok(f)
}

pub struct DownloadTarget {
    pub url: String,
    pub path: String,
    pub checksum: Option<String>,
    pub ext: String,
    pub executable: bool, // if target is a executable file not a tarball
}

fn get_formula_git_url(package_name: &str) -> String {
    format!("https://{}.git", package_name)
}

fn print_publishing_msg() {
    let msg = r#"It looks like the package does not support Cask
                        If you are the package owner, see our documentation for how to publish a package:
                        https://github.com/cask-pkg/cask.rs/blob/main/DESIGN.md#how-do-i-publish-package"
                    "#
    .lines()
    .map(|s| s.trim_start().to_owned())
    .collect::<Vec<String>>()
    .join("\n");

    eprintln!("{}", msg);
}

pub fn fetch(
    cask: &cask::Cask,
    package_name: &str,
    temp: bool,
    is_verbose: bool,
) -> Result<Formula, Report> {
    eprintln!("Fetching {} formula...", package_name);

    if let Ok(package_addr) = Url::parse(package_name) {
        let scheme = package_addr.scheme();
        return match scheme {
            "http" | "https" => {
                let repo = git::new(package_addr.as_str())?;
                let is_package_repo_exist = repo.is_exist()?;

                if is_package_repo_exist {
                    fetch_with_git_url(cask, package_name, package_addr.as_str(), temp, is_verbose)
                } else {
                    Err(eyre::format_err!(
                        "The package '{}' does not exist!",
                        package_name
                    ))
                }
            }
            _ => Err(eyre::format_err!(
                "Not support the protocol '{}' of package address.",
                scheme
            )),
        };
    }

    let fo = find_package_in_build_in(cask, package_name)?;

    if let Some(f) = fo {
        Ok(f)
    } else {
        let package_repo_url = get_formula_git_url(package_name);

        let is_repo_exist = git::new(&package_repo_url)?.is_exist()?;

        if is_repo_exist {
            fetch_with_git_url(cask, package_name, &package_repo_url, temp, is_verbose)
        } else {
            Err(eyre::format_err!("can not found package {}", package_name))
        }
    }
}

fn find_package_in_build_in(
    cask: &cask::Cask,
    package_name: &str,
) -> Result<Option<Formula>, Report> {
    // try found package in build-in
    let mut build_in_dir = cask.build_in_formula_dir();

    if !build_in_dir.exists() {
        return Ok(None);
    }

    for p in package_name.split('/') {
        build_in_dir = build_in_dir.join(p)
    }

    let cask_file_path = build_in_dir.join("Cask.toml");

    if cask_file_path.exists() {
        return new(&cask_file_path, "").map(Some);
    }

    Ok(None)
}

// fetch remote formula
fn fetch_with_git_url(
    cask: &cask::Cask,
    package_name: &str,
    git_url: &str,
    temp: bool,
    is_verbose: bool,
) -> Result<Formula, Report> {
    let unix_time = {
        let start = SystemTime::now();

        let t = start.duration_since(UNIX_EPOCH)?;

        t.as_secs()
    };

    let formula_cloned_dir = {
        if temp {
            env::temp_dir().join(format!("cask_formula_{}", unix_time))
        } else {
            cask.package_dir(package_name).join("repository")
        }
    };

    if formula_cloned_dir.exists() {
        fs::remove_dir_all(&formula_cloned_dir)?;
    }

    let cask_file_path = formula_cloned_dir.join("Cask.toml");

    match git::new(git_url)?.clone(
        &formula_cloned_dir,
        git::CloneOption {
            depth: Some(1),
            quiet: Some(!is_verbose),
            verbose: Some(is_verbose),
            progress: Some(!is_verbose),
            single_branch: Some(true),
            dissociate: Some(true),
            filter: Some("tree:0".to_string()),
        },
    ) {
        Ok(()) => {
            if !cask_file_path.exists() {
                print_publishing_msg();

                return Err(eyre::format_err!(
                    "{} is not a valid formula!",
                    package_name
                ));
            }

            match new(&cask_file_path, git_url) {
                Ok(r) => {
                    if temp {
                        fs::remove_dir_all(formula_cloned_dir)?;
                    }
                    Ok(r)
                }
                Err(e) => {
                    if temp {
                        fs::remove_dir_all(formula_cloned_dir)?;
                    }
                    Err(e)
                }
            }
        }
        Err(e) => Err(eyre::format_err!("{}", e)),
    }
}

impl Formula {
    fn get_current_os(&self) -> Option<&Platform> {
        if cfg!(target_os = "macos") {
            self.darwin.as_ref()
        } else if cfg!(target_os = "windows") {
            self.windows.as_ref()
        } else if cfg!(target_os = "linux") {
            self.linux.as_ref()
        } else {
            None
        }
    }
    fn get_current_arch(&self) -> Option<&ResourceTarget> {
        if let Some(os) = self.get_current_os() {
            if cfg!(target_arch = "x86") {
                os.x86.as_ref()
            } else if cfg!(target_arch = "x86_64") {
                os.x86_64.as_ref()
            } else if cfg!(target_arch = "arm") {
                os.arm.as_ref()
            } else if cfg!(target_arch = "armv7") {
                os.armv7.as_ref()
            } else if cfg!(target_arch = "aarch64") {
                os.aarch64.as_ref()
            } else if cfg!(target_arch = "mips") {
                os.mips.as_ref()
            } else if cfg!(target_arch = "mips64") {
                os.mips64.as_ref()
            } else if cfg!(target_arch = "mips64el") {
                os.mips64el.as_ref()
            } else if cfg!(target_arch = "riscv64") {
                os.riscv64.as_ref()
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn get_file_content(&self) -> String {
        self.file_content.clone()
    }

    pub fn get_current_download_url(&self, version: &str) -> Result<DownloadTarget, Report> {
        #[derive(Serialize)]
        struct URLTemplateContext<'a> {
            version: &'a str,
            package: &'a Package,
            context: Option<&'a HashMap<String, String>>,
        }

        if let Some(resource_target) = self.get_current_arch() {
            let render_context = URLTemplateContext {
                version,
                package: &self.package,
                context: self.context.as_ref(),
            };

            let mut tt = TinyTemplate::new();

            let download_url = match resource_target {
                ResourceTarget::Detailed(detail) => detail.url.clone(),
                ResourceTarget::Executable(exe) => exe.executable.clone(),
                ResourceTarget::Simple(url) => url.to_string(),
            };

            tt.add_template("url_template", &download_url)?;

            let renderer_url = tt.render("url_template", &render_context)?;

            let get_ext_name_from_url = || -> Result<&str, Report> {
                let u = Url::parse(&renderer_url)?;

                let default_ext = extractor::Extension::TarGz;

                if let Some(sep) = u.path_segments() {
                    let filename = sep.last().unwrap_or(default_ext.as_str());

                    if filename.ends_with(extractor::Extension::TarGz.as_str()) {
                        Ok(extractor::Extension::TarGz.as_str())
                    } else if filename.ends_with(extractor::Extension::Tgz.as_str()) {
                        Ok(extractor::Extension::Tgz.as_str())
                    } else if filename.ends_with(extractor::Extension::Tar.as_str()) {
                        Ok(extractor::Extension::Tar.as_str())
                    } else if filename.ends_with(extractor::Extension::Zip.as_str()) {
                        Ok(extractor::Extension::Zip.as_str())
                    } else {
                        Ok(default_ext.as_str())
                    }
                } else {
                    Ok(default_ext.as_str())
                }
            };

            let mut path = match resource_target {
                ResourceTarget::Detailed(arch) => arch.path.clone(),
                ResourceTarget::Executable(_) => None,
                ResourceTarget::Simple(_) => None,
            }
            .unwrap_or_else(|| "/".to_string());

            if path.trim().is_empty() {
                path = "/".to_string();
            }

            tt.add_template("path_template", &path)?;

            path = tt.render("path_template", &render_context)?;

            let ext_name = match resource_target {
                ResourceTarget::Detailed(arch) => match &arch.extension {
                    Some(ext) => ext.as_str().to_string(),
                    None => get_ext_name_from_url()?.to_string(),
                },
                ResourceTarget::Executable(_) => {
                    #[cfg(unix)]
                    {
                        "".to_string()
                    }
                    #[cfg(windows)]
                    {
                        ".exe".to_string()
                    }
                }
                ResourceTarget::Simple(_) => get_ext_name_from_url()?.to_string(),
            };

            let checksum = match resource_target {
                ResourceTarget::Detailed(arch) => arch.checksum.clone(),
                ResourceTarget::Executable(arch) => arch.checksum.clone(),
                ResourceTarget::Simple(_) => None,
            };

            Ok(DownloadTarget {
                url: renderer_url,
                path: path.trim().to_string(),
                checksum,
                ext: ext_name,
                executable: matches!(resource_target, ResourceTarget::Executable(_)),
            })
        } else {
            Err(eyre::format_err!(
                "the package '{}' not support your system",
                self.package.name
            ))
        }
    }

    // get all remote versions
    pub fn get_versions(&self) -> Result<Vec<String>, Report> {
        if let Some(versions) = &self.package.versions {
            Ok(versions.to_vec())
        } else {
            git::new(&self.package.repository)?
                .versions()
                .map_err(|e| eyre::format_err!("{}", e))
        }
    }

    // get the latest version of package
    pub fn get_latest_version(&self) -> Result<Option<String>, Report> {
        let version = self.get_versions()?;

        Ok(version.first().map(|f| f.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use std::env;

    use crate::cask;
    use crate::formula;

    #[test]
    fn test_read_default_config() {
        let config_path = env::current_dir()
            .unwrap()
            .join("fixtures")
            .join("config")
            .join("default_Cask.toml");

        let rc = formula::new(&config_path, "https://github.com/example/example.git").unwrap();

        assert_eq!(rc.repository, "https://github.com/example/example.git");
        assert_eq!(
            format!("{}", rc.filepath.display()),
            format!("{}", config_path.display())
        );
        assert_eq!(rc.package.name, "github.com/axetroy/gpm.rs");
        assert_eq!(rc.package.bin, "gpm");
        assert_eq!(
            rc.package.versions.as_ref().unwrap(),
            &vec!["0.1.12", "0.1.11"]
        );
        assert_eq!(
            rc.package.authors.as_ref().unwrap(),
            &vec!["Axetroy <axetroy.dev@gmail.com>"]
        );
        assert_eq!(
            rc.package.keywords.as_ref().unwrap(),
            &vec!["gpm", "git", "project", "manager"]
        );
        assert_eq!(rc.package.repository, "https://github.com/axetroy/gpm.rs");
        assert_eq!(
            rc.package.description,
            "A command line tool, manage your hundreds of repository, written with Rust.\n"
        );

        let windows = &rc.windows.as_ref().unwrap();
        let darwin = &rc.darwin.as_ref().unwrap();
        let linux = &rc.linux.as_ref().unwrap();

        // windows
        match windows.x86_64.as_ref().unwrap() {
            formula::ResourceTarget::Detailed(arch) => {
                assert_eq!(
                    arch.url,
                    "{package.repository}/releases/download/v{version}/gpm_windows_amd64.tar.gz"
                );
            }
            formula::ResourceTarget::Executable(_) => todo!(),
            formula::ResourceTarget::Simple(_) => todo!(),
        }

        // darwin
        match darwin.x86_64.as_ref().unwrap() {
            formula::ResourceTarget::Detailed(arch) => {
                assert_eq!(
                    arch.url,
                    "{package.repository}/releases/download/v{version}/gpm_darwin_amd64.tar.gz"
                );
            }
            formula::ResourceTarget::Executable(_) => todo!(),
            formula::ResourceTarget::Simple(_) => todo!(),
        }
        match darwin.aarch64.as_ref().unwrap() {
            formula::ResourceTarget::Detailed(arch) => {
                assert_eq!(
                    arch.url,
                    "{package.repository}/releases/download/v{version}/gpm_darwin_arm64.tar.gz"
                );
            }
            formula::ResourceTarget::Executable(_) => todo!(),
            formula::ResourceTarget::Simple(_) => todo!(),
        }

        #[cfg(target_os = "macos")]
        assert_eq!(
            &rc.get_current_download_url("0.1.12").as_ref().unwrap().url,
            "https://github.com/axetroy/gpm.rs/releases/download/v0.1.12/gpm_darwin_amd64.tar.gz"
        );

        // linux
        match linux.x86_64.as_ref().unwrap() {
            formula::ResourceTarget::Detailed(arch) => {
                assert_eq!(
                    arch.url,
                    "{package.repository}/releases/download/v{version}/gpm_linux_amd64.tar.gz"
                );
            }
            formula::ResourceTarget::Executable(_) => todo!(),
            formula::ResourceTarget::Simple(_) => todo!(),
        }
        match linux.aarch64.as_ref().unwrap() {
            formula::ResourceTarget::Detailed(arch) => {
                assert_eq!(
                    arch.url,
                    "{package.repository}/releases/download/v{version}/gpm_linux_arm64.tar.gz"
                );
            }
            formula::ResourceTarget::Executable(_) => todo!(),
            formula::ResourceTarget::Simple(_) => todo!(),
        }
    }

    #[test]
    fn test_read_simple_config() {
        let config_path = env::current_dir()
            .unwrap()
            .join("fixtures")
            .join("config")
            .join("simple_Cask.toml");

        let rc = formula::new(&config_path, "https://github.com/example/example.git").unwrap();

        assert_eq!(rc.repository, "https://github.com/example/example.git");
        assert_eq!(
            format!("{}", rc.filepath.display()),
            format!("{}", config_path.display())
        );
        assert_eq!(rc.package.name, "github.com/axetroy/gpm.rs");
        assert_eq!(rc.package.bin, "gpm");
        assert_eq!(rc.package.versions.unwrap(), vec!["0.1.12", "0.1.11"]);
        assert_eq!(
            rc.package.authors.unwrap(),
            vec!["Axetroy <axetroy.dev@gmail.com>"]
        );
        assert_eq!(
            rc.package.keywords.unwrap(),
            vec!["gpm", "git", "project", "manager"]
        );
        assert_eq!(rc.package.repository, "https://github.com/axetroy/gpm.rs");
        assert_eq!(
            rc.package.description,
            "A command line tool, manage your hundreds of repository, written with Rust.\n"
        );

        let windows = &rc.windows.unwrap();
        let darwin = &rc.darwin.unwrap();
        let linux = &rc.linux.unwrap();

        // windows
        match windows.x86_64.as_ref().unwrap() {
            formula::ResourceTarget::Detailed(_) => todo!(),
            formula::ResourceTarget::Executable(_) => todo!(),
            formula::ResourceTarget::Simple(url) => {
                assert_eq!(
                    url,
                    "https://github.com/axetroy/gpm.rs/releases/download/v{version}/gpm_windows_amd64.tar.gz"
                )
            }
        }

        // darwin
        match darwin.x86_64.as_ref().unwrap() {
            formula::ResourceTarget::Detailed(_) => todo!(),
            formula::ResourceTarget::Executable(_) => todo!(),
            formula::ResourceTarget::Simple(url) => {
                assert_eq!(
                    url,
                    "https://github.com/axetroy/gpm.rs/releases/download/v{version}/gpm_darwin_amd64.tar.gz"
                )
            }
        }
        match darwin.aarch64.as_ref().unwrap() {
            formula::ResourceTarget::Detailed(_) => todo!(),
            formula::ResourceTarget::Executable(_) => todo!(),
            formula::ResourceTarget::Simple(url) => {
                assert_eq!(
                    url,
                    "https://github.com/axetroy/gpm.rs/releases/download/v{version}/gpm_darwin_arm64.tar.gz"
                )
            }
        }

        // linux
        match linux.x86_64.as_ref().unwrap() {
            formula::ResourceTarget::Detailed(_) => todo!(),
            formula::ResourceTarget::Executable(_) => todo!(),
            formula::ResourceTarget::Simple(url) => {
                assert_eq!(
                    url,
                    "https://github.com/axetroy/gpm.rs/releases/download/v{version}/gpm_linux_amd64.tar.gz"
                )
            }
        }
        match linux.aarch64.as_ref().unwrap() {
            formula::ResourceTarget::Detailed(_) => todo!(),
            formula::ResourceTarget::Executable(_) => todo!(),
            formula::ResourceTarget::Simple(url) => {
                assert_eq!(
                    url,
                    "https://github.com/axetroy/gpm.rs/releases/download/v{version}/gpm_linux_arm64.tar.gz"
                )
            }
        }
    }

    #[test]
    fn test_fetch_from_git_url() {
        let root_dir = env::current_dir().unwrap().join("fixtures").join(".cask");
        let c = cask::new(&root_dir);

        let formula =
            formula::fetch(&c, "https://github.com/axetroy/prune.v", true, false).unwrap();

        assert_eq!(formula.package.name, "github.com/axetroy/prune.v")
    }

    #[test]
    fn test_get_hook() {
        let config_path = env::current_dir()
            .unwrap()
            .join("fixtures")
            .join("config")
            .join("hook_Cask.toml");

        let rc = formula::new(&config_path, "https://github.com/example/example.git").unwrap();

        let terminal_hook = rc.hook.as_ref().unwrap().resolve().unwrap();

        if cfg!(target_family = "unix") {
            assert_eq!(terminal_hook.terminal, shell::Terminal::Sh);
            assert_eq!(
                terminal_hook.hook.preinstall.as_ref().unwrap(),
                r#"echo 'hello sh'"#
            );
        } else {
            assert_eq!(terminal_hook.terminal, shell::Terminal::Cmd);
            assert_eq!(
                terminal_hook.hook.preinstall.as_ref().unwrap(),
                r#"echo 'hello cmd'"#
            );
        }
    }
}
