#[macro_use]
extern crate clap;
extern crate ansi_term;
extern crate atty;
extern crate regex;
extern crate ignore;

pub mod lscolors;
pub mod fshelper;

mod utils;

use utils::IntoInits;

use std::borrow::Borrow;
use std::borrow::Cow;
use std::env;
use std::error::Error;
use std::io;
use std::io::{Write, BufWriter};
#[cfg(target_family = "unix")]
use std::os::unix::fs::PermissionsExt;
use std::path::{PathBuf, Path, Component};
use std::process;

use clap::{App, AppSettings, Arg};
use atty::Stream;
use regex::{Match, Regex, RegexBuilder};
use ignore::WalkBuilder;

use lscolors::LsColors;

/// Defines how to display search result paths.
#[derive(PartialEq)]
enum PathDisplay {
    /// As an absolute path
    Absolute,

    /// As a relative path
    Relative
}

/// Configuration options for *fd*.
struct FdOptions {
    /// Determines whether the regex search is case-sensitive or case-insensitive.
    case_sensitive: bool,

    /// Whether to search within the full file path or just the base name (filename or directory
    /// name).
    search_full_path: bool,

    /// Whether to ignore hidden files and directories (or not).
    ignore_hidden: bool,

    /// Whether to respect VCS ignore files (`.gitignore`, `.ignore`, ..) or not.
    read_ignore: bool,

    /// Whether to follow symlinks or not.
    follow_links: bool,

    /// Whether elements of output should be separated by a null character
    null_separator: bool,

    /// The maximum search depth, or `None` if no maximum search depth should be set.
    ///
    /// A depth of `1` includes all files under the current directory, a depth of `2` also includes
    /// all files under subdirectories of the current directory, etc.
    max_depth: Option<usize>,

    /// Display results as relative or absolute path.
    path_display: PathDisplay,

    /// `None` if the output should not be colorized. Otherwise, a `LsColors` instance that defines
    /// how to style different filetypes.
    ls_colors: Option<LsColors>
}

/// Path separator (taken from ::sys::path::MAIN_SEP_STR)
#[cfg(target_family = "unix")]
static MAIN_SEPARATOR : &'static str = "/";
#[cfg(not(target_family = "unix"))]
static MAIN_SEPARATOR : &'static str = "\\";

/// Root directory
#[cfg(target_family = "unix")]
static ROOT_DIR : &'static str = "/";
#[cfg(not(target_family = "unix"))]
static ROOT_DIR : &'static str = "\\";

/// Parent directory
static PARENT_DIR : &'static str = "..";

/// Current directory
static CURRENT_DIR : &'static str = ".";

fn component_to_str<'a>(component: Component<'a>) -> Cow<'a, str> {
    match component {
        Component::Prefix(p) => p.as_os_str().to_string_lossy(),
        Component::RootDir   => Cow::Borrowed(ROOT_DIR),
        Component::CurDir    => Cow::Borrowed(CURRENT_DIR),
        Component::ParentDir => Cow::Borrowed(PARENT_DIR),
        Component::Normal(p) => p.to_string_lossy(),
    }
}

/// Print a search result to the console.
fn display_entry<'a>(path: &'a Path, matching: Match, ls_colors: &Option<LsColors>) -> Cow<'a, str> {
    if let &Some(ref ls_colors) = ls_colors {
        display_styled_entry(path, matching, ls_colors)
    } else {
        path.to_string_lossy()
    }
}

fn display_styled_entry<'a>(path: &'a Path, matching: Match, ls_colors: &LsColors) -> Cow<'a, str> {
    let (match_start, match_end) = (matching.start(), matching.end());

    // Get each path component as a string
    let component_strs: Vec<_> = path.components()
        .map(component_to_str)
        .collect();

    // Get each path component's full path
    let component_paths: Vec<_> = component_strs.iter()
        .inits()
        .map(|ss| {
            let v: Vec<_> = ss.into_iter()
                .map(|s| s.borrow())
                .collect();

            v.join(MAIN_SEPARATOR)
        })
        .map(|s| PathBuf::from(s))
        .collect();

    // For each path component, retrieve the appropriate style using the full path, and style
    // the component's string accordingly, optionally underlining the section that's in the
    // match.
    let styled_strs = component_paths.iter()
        .map(|p| get_path_style(&ls_colors, &p))
        .zip(component_strs.iter());

    let output = styled_strs
        .map(|(style, s)| style.paint(s.to_string()).to_string())
        .collect::<Vec<_>>()
        .join(&ls_colors.directory.paint(MAIN_SEPARATOR).to_string());

    Cow::Owned(output)
}

// path -> (base, entry)
fn display_styled_entry_0(base: &Path, entry: &Path, matching: Match, ls_colors: &LsColors) -> String {
    let path_full = base.join(entry);
    let mut component_path = base.to_path_buf();

    let mut display = String::new();

    for component in entry.components() {
        let comp_str = component_to_str(component);

        component_path.push(Path::new(&*comp_str));

        let style = get_path_style(ls_colors, &component_path);

        display += &style.paint(comp_str).to_string();

        if component_path.is_dir() && component_path != path_full {
            display += &style.paint(MAIN_SEPARATOR).to_string();
        }
    }

    display
}

#[cfg(target_family = "unix")]
fn is_executable(p: &Path) -> bool {
    p.metadata()
        .ok()
        .map(|f| f.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(target_family = "unix"))]
fn is_executable(_: &Path) -> bool {
    false
}

fn is_symlink(path: &Path) -> bool {
    path.symlink_metadata()
        .map(|md| md.file_type().is_symlink())
        .unwrap_or(false)
}

fn get_path_style<'a>(ls_colors: &'a LsColors, path: &Path) -> Cow<'a, ansi_term::Style> {
    if is_symlink(path) {
        Cow::Borrowed(&ls_colors.symlink)
    } else if path.is_dir() {
        Cow::Borrowed(&ls_colors.directory)
    } else if is_executable(&path) {
        Cow::Borrowed(&ls_colors.executable)
    } else {
        path.file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| ls_colors.filenames.get(n))
            .map(Cow::Borrowed)
            .or_else(|| {
                path.extension()
                    .and_then(|e| e.to_str())
                    .and_then(|e| ls_colors.extensions.get(e))
                    .map(Cow::Borrowed)
            })
            .unwrap_or_default()
    }
}

/// Recursively scan the given search path and search for files / pathnames matching the pattern.
fn scan(root: &Path, pattern: &Regex, base: &Path, config: &FdOptions) {
    let walker = WalkBuilder::new(root)
                     .hidden(config.ignore_hidden)
                     .ignore(config.read_ignore)
                     .git_ignore(config.read_ignore)
                     .parents(config.read_ignore)
                     .git_global(config.read_ignore)
                     .git_exclude(config.read_ignore)
                     .follow_links(config.follow_links)
                     .max_depth(config.max_depth)
                     .build()
                     .into_iter()
                     .filter_map(|e| e.ok())
                     .filter(|e| e.path() != root);

    let output = io::stdout();
    let mut writer = BufWriter::new(output.lock());

    for entry in walker {
        let path = entry.path();
        let path_rel = fshelper::path_relative_from(path, base)
            .unwrap_or_else(|| {
                error("Error: could not get relative path for directory entry.")
            });

        let search_str_o =
            if config.search_full_path {
                Some(path_rel.to_string_lossy())
            } else {
                path_rel.file_name()
                    .map(|f| f.to_string_lossy())
            };

        if let Some(search_str) = search_str_o {
            let search_match = pattern.find(&*search_str);
            if let Some(matching) = search_match {
                let path =
                    if config.path_display != PathDisplay::Absolute {
                        &path_rel
                    } else {
                        path
                    };

                let s = display_entry(path, matching, &config.ls_colors);

                let separator = if config.null_separator { '\0' } else { '\n' };
                write!(&mut writer, "{}{}", s, separator)
                    .expect("Failed writing to stdout");
            }
        }
    }
}

/// Print error message to stderr and exit with status `1`.
fn error(message: &str) -> ! {
    writeln!(&mut io::stderr(), "{}", message)
        .expect("Failed writing to stderr");
    process::exit(1);
}

fn main() {
    let matches =
        App::new("fd")
            .version(crate_version!())
            .usage("fd [FLAGS/OPTIONS] [<pattern>] [<path>]")
            .setting(AppSettings::ColoredHelp)
            .setting(AppSettings::DeriveDisplayOrder)
            .arg(Arg::with_name("case-sensitive")
                        .long("case-sensitive")
                        .short("s")
                        .help("Case-sensitive search (default: smart case)"))
            .arg(Arg::with_name("full-path")
                        .long("full-path")
                        .short("p")
                        .help("Search full path (default: file-/dirname only)"))
            .arg(Arg::with_name("hidden")
                        .long("hidden")
                        .short("H")
                        .help("Search hidden files and directories"))
            .arg(Arg::with_name("no-ignore")
                        .long("no-ignore")
                        .short("I")
                        .help("Do not respect .(git)ignore files"))
            .arg(Arg::with_name("follow")
                        .long("follow")
                        .short("f")
                        .help("Follow symlinks"))
            .arg(Arg::with_name("null_separator")
                        .long("print0")
                        .short("0")
                        .help("Separate results by the null character"))
            .arg(Arg::with_name("absolute-path")
                        .long("absolute-path")
                        .short("a")
                        .help("Show absolute instead of relative paths"))
            .arg(Arg::with_name("no-color")
                        .long("no-color")
                        .short("n")
                        .help("Do not colorize output"))
            .arg(Arg::with_name("depth")
                        .long("max-depth")
                        .short("d")
                        .takes_value(true)
                        .help("Set maximum search depth (default: none)"))
            .arg(Arg::with_name("pattern")
                        .help("the search pattern, a regular expression (optional)"))
            .arg(Arg::with_name("path")
                        .help("the root directory for the filesystem search (optional)"))
            .get_matches();

    // Get the search pattern
    let empty_pattern = String::new();
    let pattern = matches.value_of("pattern").unwrap_or(&empty_pattern);

    // Get the current working directory
    let current_dir_buf = match env::current_dir() {
        Ok(cd) => cd,
        Err(_) => error("Error: could not get current directory.")
    };
    let current_dir = current_dir_buf.as_path();

    // Get the root directory for the search
    let mut root_dir_is_absolute = false;
    let root_dir_buf = if let Some(rd) = matches.value_of("path") {
        let path = Path::new(rd);

        root_dir_is_absolute = path.is_absolute();

        path.canonicalize().unwrap_or_else(
            |_| error(&format!("Error: could not find directory '{}'.", rd))
        )
    } else {
        current_dir_buf.clone()
    };

    if !root_dir_buf.is_dir() {
        error(&format!("Error: '{}' is not a directory.", root_dir_buf.to_string_lossy()));
    }

    let root_dir = root_dir_buf.as_path();

    // The search will be case-sensitive if the command line flag is set or
    // if the pattern has an uppercase character (smart case).
    let case_sensitive = matches.is_present("case-sensitive") ||
                         pattern.chars().any(char::is_uppercase);

    let colored_output = !matches.is_present("no-color") &&
                         atty::is(Stream::Stdout);

    let ls_colors =
        if colored_output {
            Some(
                env::var("LS_COLORS")
                    .ok()
                    .map(|val| LsColors::from_string(&val))
                    .unwrap_or_default()
            )
        } else {
            None
        };

    let config = FdOptions {
        case_sensitive:    case_sensitive,
        search_full_path:  matches.is_present("full-path"),
        ignore_hidden:     !matches.is_present("hidden"),
        read_ignore:       !matches.is_present("no-ignore"),
        follow_links:      matches.is_present("follow"),
        null_separator:    matches.is_present("null_separator"),
        max_depth:         matches.value_of("depth")
                                   .and_then(|ds| usize::from_str_radix(ds, 10).ok()),
        path_display:      if matches.is_present("absolute-path") || root_dir_is_absolute {
                               PathDisplay::Absolute
                           } else {
                               PathDisplay::Relative
                           },
        ls_colors:         ls_colors
    };

    let root = Path::new(ROOT_DIR);
    let base = match config.path_display {
        PathDisplay::Relative => current_dir,
        PathDisplay::Absolute => root
    };

    match RegexBuilder::new(pattern)
              .case_insensitive(!config.case_sensitive)
              .build() {
        Ok(re)   => scan(root_dir, &re, base, &config),
        Err(err) => error(err.description())
    }
}
