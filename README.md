# Stencil

Stencil is a templating program allowing multiple files to be updated according to a template and replacement set. The common use case is color coordination: many configuration files can be created as templates, and stencil will replace the template markers in each file with a color defined in a central location. This allows many programs to follow the same color scheme. Multiple "replacement sets" can be defined, allowing different color themes to be swapped in and out.

```console
user@name ~ $ stencil --help
Stencil 0.1
Liam Pribis <jackpribis@gmail.com>
System-wide templater

USAGE:
    stencil [FLAGS] [OPTIONS]

FLAGS:
    -h, --help         Prints help information
        --list-sets    list the replacement sets in the current config file
    -q, --quiet        supress output
    -V, --version      Prints version information
    -v                 verbose output

OPTIONS:
    -c, --config <CONFIG_DIR_PATH>    set the path of the configuration directory
    -r, --run <SET>                   run stencil with a given replacement set
```

## Configuration directory
By default, stencil looks in its working directory for a `config.toml`. The configuration directory can be overridden using the `-c <DIRECTORY>` command line flag. The configuration directory contains all templates and backed up files along with the `config.toml`.

## config.toml
### Top level options
* **template-before**: *string*, sets the marker that comes before a templated value.
* **template-after**: *string*, sets the marker that comes after a templated value.
* **backup**: *optional boolean*, determines whether files will be backed up in `$CONFIG_DIRECTORY/backup/` before they are replaced by the templater. If this option is not present, it defaults to `true`.

For example:
```toml
template-before = "!TEMPLATE("
template-after = ")"
```
will search for substrings of the form `!TEMPLATE(name)`, and replace them with the specified string associated with `name` in the replacement set.

### Specifying files to be templated
Files are defined as an array of tables in toml. Each file declaration has the form
```toml
[[file]]
path = "/home/user/.config/a-program/config.cfg"
template = "a-program.template"
whitelist = ["set-a", "set-b"]
```
* **path**: *string*, the full path where a templated output file should be stored.
* **template**: *string*, the filename of the template file. This file should exist in the configuration directory. Eg. if `template = "foo.template"`, `$CONFIG_DIRECTORY/foo.template` should exist.
* **whitelist**: *optional list of strings*, if this option is present, only replacement sets in the whitelist will be applied to this file. If a replacement set is run that is not in this list, this file will be skipped for templating.

### Specifying replacement sets
Replacement sets are a mapping from a name to a replacement string. In TOML, they are represented by a table of tables. Each replacement set has the form
```toml
[set.name]
whitelist-only = true
key-1 = "value 1"
key-2 = "value 2"
key-n = "value n"
```
* **name**: *string*, the name of this replacement set, used in file whitelists and when running `stencil -r <NAME>`
* **whitelist-only**: *optional boolean*, if this option is set to `true`, this replacement set will only apply to files that have specifically whitelisted this replacement set. Otherwise it will be applied to any file that either whitelists it or has no specified whitelist. If not present, it defaults to `false`.
* **key/value pairs**: *string to string map*, defines which names in a template will be substituted for which vales.