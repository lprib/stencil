use std::{
    collections::HashMap, fmt::Display, fs, io::ErrorKind, io::Write, ops::Deref, path::Path,
    path::PathBuf,
};

use clap::{App, AppSettings, Arg, ArgMatches};
use fs::OpenOptions;
use once_cell::sync::OnceCell;
use regex::Regex;
use serde::Deserialize;
use toml::de::Error;

type StringResult<T = ()> = Result<T, String>;

#[derive(Deserialize, Debug)]
struct Config {
    #[serde(rename = "template-before")]
    before: String,
    #[serde(rename = "template-after")]
    after: String,
    #[serde(rename = "file")]
    files: Vec<TemplatedFile>,
    #[serde(rename = "set")]
    sets: HashMap<String, ReplacementSet>,
    #[serde(default = "default_backup_value")]
    backup: bool,
}

#[derive(Deserialize, Debug)]
struct ReplacementSet {
    #[serde(rename = "whitelist-only", default = "default_whitelist_only")]
    whitelist_only: bool,
    #[serde(flatten)]
    mapping: HashMap<String, String>,
}

#[derive(Deserialize, Debug)]
struct TemplatedFile {
    #[serde(rename = "path")]
    output_path: String,
    template: String,
    #[serde(rename = "whitelist")]
    whitelisted_sets: Option<Vec<String>>,
}
#[derive(Debug, PartialOrd, PartialEq)]
enum LogLevel {
    None,
    Warn,
    Trace,
}

static LOG_LEVEL: OnceCell<LogLevel> = OnceCell::new();

/// Handle errors based on the loglevel in the real main function, `main_err`
fn main() {
    match main_err() {
        Err(e) if LOG_LEVEL.get().unwrap() > &LogLevel::None => println!("[Error] {}", e),
        _ => {}
    }
}

/// The main logic of stencil
fn main_err() -> StringResult {
    let matches = get_cli_args();

    LOG_LEVEL
        .set(if matches.is_present("verbose") {
            LogLevel::Trace
        } else if matches.is_present("quiet") {
            LogLevel::None
        } else {
            LogLevel::Warn
        })
        .unwrap();

    let (config_folder, config) = get_config(&matches)?;

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
            backup_files(&config, &config_folder)?;
        }

        let replace_regex = get_replacement_regex(&config);

        // unwrapping here is ok because it is checked above
        let whitelist_only = config.sets.get(set_name).unwrap().whitelist_only;

        for file in &config.files {
            if let Some(ref whitelist) = file.whitelisted_sets {
                if !whitelist.iter().any(|set| set == set_name) {
                    log(
                        format!(
                            "set `{}` is  not whitelisted for file `{}`, skipping",
                            set_name, file.output_path
                        ),
                        LogLevel::Trace,
                    );
                    continue;
                }
            } else if whitelist_only {
                // there is no whitelist for file, but the set requires whitelists only, so skip
                log(
                    format!(
                        "set `{}` requires whitelist only, but `{}` does not specify a whitelist, skipping",
                        set_name, file.output_path
                    ),
                LogLevel::Trace
                );
                continue;
            }

            match replace_file(file, &config, &set_name, &replace_regex, &config_folder) {
                // dont abort program on error, just continue to next file
                Err(e) => log(e, LogLevel::Warn),
                _ => {}
            }
        }
    }

    Ok(())
}

fn get_cli_args() -> ArgMatches<'static> {
    App::new("Stencil")
        .version("1.0")
        .author("Liam Pribis <jackpribis@gmail.com>")
        .about("System-wide templater")
        .setting(AppSettings::ArgRequiredElseHelp)
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
                .value_name("CONFIG_DIR_PATH")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("quiet")
                .long("quiet")
                .short("q")
                .conflicts_with("verbose")
                .help("supress output"),
        )
        .arg(
            Arg::with_name("verbose")
                .short("v")
                .long("verbose")
                .help("verbose output"),
        )
        .get_matches()
}

/// returns (config_directory, config) based on cli args and defaults
fn get_config(matches: &ArgMatches) -> StringResult<(PathBuf, Config)> {
    //folder where all templates, backups, and config.toml live
    let config_folder = PathBuf::from(matches.value_of("config").unwrap_or("."));
    log(
        format!(
            "using configuration directory `{}`",
            config_folder.display()
        ),
        LogLevel::Trace,
    );

    let config_toml_path = {
        let mut buf = config_folder.clone();
        buf.push("config.toml");
        buf
    };

    let config_toml_string = fs::read_to_string(&config_toml_path).map_err(display_io_error)?;
    let config: Config = toml::from_str(&config_toml_string)
        .map_err(|e| config_error_display(e, config_toml_path.as_os_str().to_str().unwrap()))?;

    Ok((config_folder, config))
}

/// Generate a Regex that finds template directives in a string, based on `template-before` and `template-after` in config.toml.
/// The Regex has a capture group named `key` that returns the text inside the template directive
fn get_replacement_regex(config: &Config) -> Regex {
    // let after = config.after.as_deref().unwrap_or(" ");
    let regex_string = format!(
        "{}(?P<key>.*?){}",
        regex::escape(&config.before),
        regex::escape(&config.after)
    );
    Regex::new(&regex_string).unwrap()
}

/// Find all templates in a given `TemplatedFile`, replaces them with the true values from `config.toml`,
/// and saves it back to the files true location.
fn replace_file(
    file: &TemplatedFile,
    config: &Config,
    set_name: &str,
    regex: &Regex,
    config_folder: &Path,
) -> StringResult {
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
        let key_match = captures.name("key").unwrap();
        let replace_str = set.mapping.get(key_match.as_str()).ok_or(format!(
            "in file `{}`:\ncould not find key `{}` in set `{}`\naborting for this file",
            template_path.to_str().unwrap_or("<non-utf8 filename>"),
            key_match.as_str(),
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
        .truncate(true)
        .open(&file.output_path)
        .map_err(display_io_error)?;
    output_file
        .write_all(output_string.as_bytes())
        .map_err(display_io_error)?;

    Ok(())
}

/// Backup all files specified in `config.toml`
fn backup_files(config: &Config, config_folder: &Path) -> StringResult {
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

/// Convert toml parsing errors into a user-readable string for reporting
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

/// required for #[serde(default = ...)]
fn default_backup_value() -> bool {
    true
}

/// required for #[serde(default = ...)]
fn default_whitelist_only() -> bool {
    false
}

/// Convert io::Error to String
fn display_io_error(e: std::io::Error) -> String {
    format!("{}", e)
}

/// Print a log message only if the defined LogLevel is high enough
#[inline]
fn log(message: String, level: LogLevel) {
    if &level <= LOG_LEVEL.get().unwrap() {
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
