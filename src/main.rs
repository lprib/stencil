use std::{
    collections::HashMap, fmt::Display, fs, io::ErrorKind, io::Write, ops::Deref, path::Path,
};

use clap::{App, Arg};
use fs::OpenOptions;
use once_cell::sync::OnceCell;
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
    #[serde(rename = "path")]
    output_path: String,
    template: String,
}
#[derive(Debug, PartialOrd, PartialEq)]
enum LogLevel {
    None,
    Warn,
    Trace,
}

static LOG_LEVEL: OnceCell<LogLevel> = OnceCell::new();

fn main() {
    match main_err() {
        Err(e) if LOG_LEVEL.get().unwrap() > &LogLevel::None => println!("[Error] {}", e),
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
        .arg(
            Arg::with_name("quiet")
                .long("quiet")
                .short("q")
                .conflicts_with("verbose")
                .help("supress output"),
        )
        .arg(Arg::with_name("verbose").short("v").help("verbose output"))
        .get_matches();

    LOG_LEVEL
        .set(if matches.is_present("verbose") {
            LogLevel::Trace
        } else if matches.is_present("quiet") {
            LogLevel::None
        } else {
            LogLevel::Warn
        })
        .unwrap();

    //todo look in directories for config or form command line arg
    //folder where all templates, backups, and config.toml live
    let config_folder = Path::new(
        matches
            .value_of("config")
            .unwrap_or("/home/liam/programming/stencil/testing/"),
    );
    log(
        format!(
            "using configuration directory `{}`",
            config_folder.display()
        ),
        LogLevel::Trace,
    );

    let config_toml_path = {
        let mut buf = config_folder.to_path_buf();
        buf.push("config.toml");
        buf
    };

    let config_toml_string = fs::read_to_string(&config_toml_path).map_err(display_io_error)?;
    let config: Config = toml::from_str(&config_toml_string)
        .map_err(|e| config_error_display(e, config_toml_path.as_os_str().to_str().unwrap()))?;

    if matches.is_present("list-sets") {
        for set_name in config.sets.keys() {
            println!("{}", set_name);
        }
        return Ok(());
    }

    if let Some(set_name) = matches.value_of("run") {
        if !config.sets.contains_key(set_name) {
            return Err(format!(
                "Set `{}` not defined in config\nThe available sets are: {}",
                set_name,
                // join available sets to string
                config
                    .sets
                    .keys()
                    .map(Deref::deref)
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
                // dont abort program on error, just continue to next file
                Err(e) => log(e, LogLevel::Warn),
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
    Regex::new(&regex_string).unwrap()
}

fn replace_file(
    file: &TemplatedFile,
    config: &Config,
    set_name: &str,
    regex: &Regex,
    config_folder: &Path,
) -> Result<(), String> {
    log(
        format!(
            "building file `{}` from template `{}`",
            file.output_path, file.template
        ),
        LogLevel::Trace,
    );

    // ok to unwrap because key existence is already checked in caller
    let set = config.sets.get(set_name).unwrap();
    let template_path = {
        let mut buf = config_folder.to_path_buf();
        buf.push(&file.template);
        buf
    };
    let template_string = fs::read_to_string(&template_path).map_err(display_io_error)?;
    let mut output_string = String::new();

    // the index of the tail after the previous match
    let mut tail_idx = 0;
    for captures in regex.captures_iter(&template_string) {
        // full_match includes the template-before and template-after strings
        let full_match = captures.get(0).unwrap();
        let inner_match = captures.get(1).unwrap();
        let replace_str = set.get(inner_match.as_str()).ok_or(format!(
            "in file `{}`:\ncould not find key `{}` in set `{}`\naborting for this file",
            template_path.to_str().unwrap_or("<non-utf8 filename>"),
            inner_match.as_str(),
            set_name
        ))?;

        output_string.push_str(&template_string[tail_idx..full_match.start()]);
        output_string.push_str(replace_str);

        tail_idx = full_match.end();
    }

    output_string.push_str(&template_string[tail_idx..]);

    let mut output_file = OpenOptions::new()
        .create(true)
        .write(true)
        .open(&file.output_path)
        .map_err(display_io_error)?;
    output_file
        .write_all(output_string.as_bytes())
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
                //alllow error if directory already exists, otherwise throw error
                Ok(())
            } else {
                Err(String::from("Failed to create backup directory"))
            }
        }
        Ok(_) => Ok(()),
    }?;

    for file in &config.files {
        let file_path = Path::new(&file.output_path)
            .canonicalize()
            .map_err(display_io_error)?;
        let mut backup_path = backup_folder.clone();
        let new_file_name = file_path
            .iter()
            // skip the "/" at the start of canonicalized name
            .skip(1)
            .map(|osstr| {
                osstr.to_str().ok_or(format!(
                    "`{}` is not a correctly formatted file path",
                    file_path.display()
                ))
            })
            .collect::<Result<Vec<&str>, String>>()?
            .join(".");
        backup_path.push(&new_file_name);

        log(
            format!(
                "Backing up `{}` into `{}`",
                &file.output_path,
                backup_path.display()
            ),
            LogLevel::Trace,
        );

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

#[inline]
fn log(message: String, level: LogLevel) {
    if &level >= LOG_LEVEL.get().unwrap() {
        println!("[{}] {}", level, message);
    }
}

impl Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::Warn => write!(f, "Warn"),
            Self::Trace => write!(f, "Trace"),
        }
    }
}
