use std::{collections::HashMap, fs, io::ErrorKind, io::Write, path::Path, path::PathBuf};

use clap::{App, Arg};
use fs::{File, OpenOptions};
use regex::Regex;
use serde::Deserialize;
use toml::de::Error;

#[derive(Deserialize, Debug)]
struct Config {
    #[serde(rename = "template-before")]
    before: String,
    #[serde(rename = "template-after")]
    after: String,
    #[serde(rename = "file")]
    files: Vec<TemplatedFile>,
    #[serde(rename = "set")]
    sets: HashMap<String, HashMap<String, String>>,
    #[serde(default = "default_backup_value")]
    backup: bool,
}

#[derive(Deserialize, Debug)]
struct TemplatedFile {
    path: String,
    template: String,
}

fn main() {
    match main_err() {
        Err(e) => println!("{}", e),
        _ => {}
    }
}

fn main_err() -> Result<(), String> {
    let matches = App::new("Stencil")
        .version("0.1")
        .author("Liam Pribis <jackpribis@gmail.com>")
        .about("System-wide templater")
        .arg(
            Arg::with_name("run")
                .long("run")
                .short("r")
                .help("run stencil with a given replacement set")
                .value_name("SET")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("list-sets")
                .long("list-sets")
                .help("list the replacement sets in the current config file"),
        )
        .arg(
            Arg::with_name("config")
                .long("config")
                .short("c")
                .help("set the path of the configuration directory")
                .value_name("CONFIG_PATH")
                .takes_value(true),
        )
        .arg(Arg::with_name("verbose").short("v").help("verbose output"))
        .get_matches();

    let config_folder = Path::new("/home/liam/programming/stencil/testing/");
    let config_file_path = {
        let mut buf = config_folder.to_path_buf();
        buf.push("config.toml");
        buf
    };

    let config_string = fs::read_to_string(&config_file_path).map_err(display_io_error)?;
    let config: Config = toml::from_str(&config_string)
        .map_err(|e| config_error_display(e, config_file_path.as_os_str().to_str().unwrap()))?;

    if let Some(set_name) = matches.value_of("run") {
        if !config.sets.contains_key(set_name) {
            return Err(format!(
                "Set `{}` not defined in config\nThe available sets are: {}",
                set_name,
                config
                    .sets
                    .keys()
                    .map(|k| &**k)
                    .collect::<Vec<&str>>()
                    .join(", ")
            ));
        }

        if config.backup {
            backup_files(&config, config_folder)?;
        }

        let replace_regex = get_replacement_regex(&config);

        for file in &config.files {
            match replace_file(file, &config, &set_name, &replace_regex, config_folder) {
                Err(e) => println!("{}", e),
                _ => {}
            }
        }
    }

    Ok(())
}

fn get_replacement_regex(config: &Config) -> Regex {
    // let after = config.after.as_deref().unwrap_or(" ");
    let regex_string = format!(
        "{}(.*?){}",
        regex::escape(&config.before),
        regex::escape(&config.after)
    );
    dbg!(&regex_string);
    Regex::new(&regex_string).unwrap()
}

fn replace_file(
    file: &TemplatedFile,
    config: &Config,
    set_name: &str,
    regex: &Regex,
    config_folder: &Path,
) -> Result<(), String> {
    // ok to unwrap because key existence is already checked in caller
    let set = config.sets.get(set_name).unwrap();
    let template_path = {
        let mut buf = config_folder.to_path_buf();
        buf.push(&file.template);
        buf
    };
    let file_string = fs::read_to_string(&template_path).map_err(display_io_error)?;
    let mut new_string = String::new();

    // the index of the tail after the last previous match
    let mut tail_idx = 0;
    for captures in regex.captures_iter(&file_string) {
        let full_match = captures.get(0).unwrap();
        let inner_match = captures.get(1).unwrap();
        let replace_str = set.get(inner_match.as_str()).ok_or(format!(
            "in file `{}`:\ncould not find key {} in set {}\naborting for this file",
            template_path.to_str().unwrap_or("<non-utf8 filename>"),
            inner_match.as_str(),
            set_name
        ))?;

        new_string.push_str(&file_string[tail_idx..full_match.start()]);
        new_string.push_str(replace_str);

        tail_idx = full_match.end();
    }

    new_string.push_str(&file_string[tail_idx..]);

    dbg!(&file.path);
    let mut output_file = OpenOptions::new()
        .create(true)
        .write(true)
        .open(&file.path)
        .map_err(display_io_error)?;
    output_file
        .write_all(new_string.as_bytes())
        .map_err(display_io_error)?;

    Ok(())
}

fn backup_files(config: &Config, config_folder: &Path) -> Result<(), String> {
    let backup_folder = {
        let mut buf = config_folder.to_path_buf();
        buf.push("backup");
        buf
    };

    match fs::create_dir(&backup_folder) {
        Err(e) => {
            if let ErrorKind::AlreadyExists = e.kind() {
                //alllow error if directory already exists, else throw error
                Ok(())
            } else {
                Err(String::from("Failed to create backup directory"))
            }
        }
        Ok(_) => Ok(()),
    }?;

    for file in &config.files {
        let file_path = Path::new(&file.path);
        let mut backup_path = backup_folder.clone();
        backup_path.push(file_path.file_name().ok_or(format!(
            "`{}` is not a correctly formatted file path",
            file_path.display()
        ))?);

        fs::copy(file_path, backup_path).map_err(display_io_error)?;
    }

    Ok(())
}

fn config_error_display(error: Error, config_path: &str) -> String {
    if let Some((line, col)) = error.line_col() {
        format!(
            "Error in configuration file `{}` at line {}, col {}:\n{}",
            config_path, line, col, error
        )
    } else {
        format!("Error in configuration file `{}`:\n{}", config_path, error)
    }
}

// required for #[serde(default = ...)]
fn default_backup_value() -> bool {
    true
}

fn display_io_error(e: std::io::Error) -> String {
    format!("{}", e)
}
