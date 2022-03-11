// #![deny(warnings)]

use eyre::Report;
use serde_derive::Deserialize;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use toml::from_str;

#[derive(Deserialize)]
pub struct Formula {
    pub package: Package,                  // The package information
    pub windows: Option<Platform>,         // The windows target information
    pub darwin: Option<Platform>,          // The macOS target information
    pub linux: Option<Platform>,           // The linux target information
    pub dependencies: Option<Vec<String>>, // The dependencies will be installed before install package
}

#[derive(Deserialize)]
pub struct Package {
    name: String,                  // The package name
    bin: String,                   // The binary name of package
    version: String,               // The version of package.
    authors: Vec<String>,          // The author of package
    keywords: Option<Vec<String>>, // The keywords of package
    repository: String,            // The repository url
    description: String,           // The description of package
}

#[derive(Deserialize)]
pub struct Platform {
    pub x86: Option<Download>,
    pub x86_64: Option<Download>,
    pub arm: Option<Download>,
    pub aarch64: Option<Download>,
    pub mips: Option<Download>,
    pub mips64: Option<Download>,
    pub mips64el: Option<Download>,
}

#[derive(Deserialize)]
pub struct Download {
    pub url: String,
}

pub fn new(config_path: &Path) -> Result<Formula, Report> {
    let mut file = match File::open(config_path) {
        Ok(f) => f,
        Err(e) => {
            return Err(eyre::Report::from(e));
        }
    };

    let mut file_content = String::new();

    match file.read_to_string(&mut file_content) {
        Ok(_) => {}
        Err(e) => {
            return Err(eyre::Report::from(e));
        }
    };

    drop(file);

    let config: Formula = match from_str(&file_content) {
        Ok(r) => r,
        Err(e) => return Err(eyre::Report::from(e)),
    };

    Ok(config)
}

#[cfg(test)]
mod tests {
    use std::env;

    use crate::formula;

    #[test]
    fn test_read_config() {
        let config_path = env::current_dir()
            .unwrap()
            .join("fixtures")
            .join("config")
            .join("default_Cask.toml");

        let rc = formula::new(&config_path).unwrap();

        assert_eq!(rc.package.name, "gpm");
        assert_eq!(rc.package.bin, "gpm");
        assert_eq!(rc.package.version, "0.1.12");
        assert_eq!(rc.package.authors, vec!["Axetroy <axetroy.dev@gmail.com>"]);
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
        assert_eq!(
            windows.x86.as_ref().unwrap().url,
            "https://github.com/axetroy/gpm.rs/releases/download/v0.1.12/gpm_windows_386.tar.gz"
        );
        assert_eq!(
            windows.x86_64.as_ref().unwrap().url,
            "https://github.com/axetroy/gpm.rs/releases/download/v0.1.12/gpm_windows_amd64.tar.gz"
        );
        assert_eq!(
            windows.aarch64.as_ref().unwrap().url,
            "https://github.com/axetroy/gpm.rs/releases/download/v0.1.12/gpm_windows_arm64.tar.gz"
        );

        // darwin
        assert_eq!(
            darwin.x86_64.as_ref().unwrap().url,
            "https://github.com/axetroy/gpm.rs/releases/download/v0.1.12/gpm_darwin_amd64.tar.gz"
        );
        assert_eq!(
            darwin.aarch64.as_ref().unwrap().url,
            "https://github.com/axetroy/gpm.rs/releases/download/v0.1.12/gpm_darwin_arm64.tar.gz"
        );

        // linux
        assert_eq!(
            linux.x86.as_ref().unwrap().url,
            "https://github.com/axetroy/gpm.rs/releases/download/v0.1.12/gpm_linux_386.tar.gz"
        );
        assert_eq!(
            linux.x86_64.as_ref().unwrap().url,
            "https://github.com/axetroy/gpm.rs/releases/download/v0.1.12/gpm_linux_amd64.tar.gz"
        );
        assert_eq!(
            linux.aarch64.as_ref().unwrap().url,
            "https://github.com/axetroy/gpm.rs/releases/download/v0.1.12/gpm_linux_arm64.tar.gz"
        );
        assert_eq!(
            linux.mips.as_ref().unwrap().url,
            "https://github.com/axetroy/gpm.rs/releases/download/v0.1.12/gpm_linux_mips.tar.gz"
        );
        assert_eq!(
            linux.mips64.as_ref().unwrap().url,
            "https://github.com/axetroy/gpm.rs/releases/download/v0.1.12/gpm_linux_mips64.tar.gz"
        );
        assert_eq!(
            linux.mips64el.as_ref().unwrap().url,
            "https://github.com/axetroy/gpm.rs/releases/download/v0.1.12/gpm_linux_mips64el.tar.gz"
        );
    }
}
