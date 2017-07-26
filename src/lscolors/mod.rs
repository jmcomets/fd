/// A parser for the `LS_COLORS` environment variable.
extern crate termcolor;

use std::collections::HashMap;

use self::termcolor::{Color, ColorSpec, StandardStream, ColorChoice, WriteColor};

use std::io;
use std::io::Write;
use std::borrow::Cow;
use std::path::Path;

/// Maps file extensions to ANSI colors / styles.
type ExtensionStyles = HashMap<String, Style>;

/// Maps filenames to ANSI colors / styles.
type FilenameStyles = HashMap<String, Style>;

const LS_CODES: &'static [&'static str] =
    &["no", "no", "fi", "rs", "di", "ln", "ln", "ln", "or", "mi", "pi", "pi",
      "so", "bd", "bd", "cd", "cd", "do", "ex", "lc", "lc", "rc", "rc", "ec",
      "ec", "su", "su", "sg", "sg", "st", "ow", "ow", "tw", "tw", "ca", "mh",
      "cl"];

/// Defines how different file system entries should be colorized / styled.
#[derive(Debug, PartialEq)]
pub struct LsColors {
    /// ANSI Style for directories.
    directory: Style,

    /// ANSI style for symbolic links.
    symlink: Style,

    /// ANSI style for executable files.
    executable: Style,

    /// A map that defines ANSI styles for different file extensions.
    extensions: ExtensionStyles,

    /// A map that defines ANSI styles for different specific filenames.
    filenames: FilenameStyles,
}

impl Default for LsColors {
    /// Get a default LsColors structure.
    fn default() -> LsColors {
        LsColors {
            directory: Color::Blue.bold(),
            symlink: Color::Cyan.normal(),
            executable: Color::Red.bold(),
            extensions: HashMap::new(),
            filenames: HashMap::new()
        }
    }
}

impl LsColors {
    /// Parse a single text-decoration code (normal, bold, italic, ...).
    fn parse_decoration(code: &str) -> Option<fn(Color) -> Style> {
        match code {
            "0" | "00" => Some(Color::normal),
            "1" | "01" => Some(Color::bold),
            "3" | "03" => Some(Color::italic),
            "4" | "04" => Some(Color::underline),
            _ => None
        }
    }

    /// Parse ANSI escape sequences like `38;5;10;1`.
    fn parse_style(code: &str) -> Option<Style> {
        let mut split = code.split(';');

        if let Some(first) = split.next() {
            // Try to match the first part as a text-decoration argument
            let mut decoration = LsColors::parse_decoration(first);

            let c1 = if decoration.is_none() { Some(first) } else { split.next() };
            let c2 = split.next();
            let c3 = split.next();

            let color =
                if c1 == Some("38") && c2 == Some("5") {
                    // TODO: support fixed colors
                    return None;
                    //let n_white = 7;
                    //let n = if let Some(num) = c3 {
                    //    u8::from_str_radix(num, 10).unwrap_or(n_white)
                    //} else {
                    //    n_white
                    //};

                    //Color::Fixed(n)
                } else if let Some(color_s) = c1 {
                    match color_s {
                        "30" => Color::Black,
                        "31" => Color::Red,
                        "32" => Color::Green,
                        "33" => Color::Yellow,
                        "34" => Color::Blue,
                        "35" => Color::Magenta, // Purple is not available?
                        "36" => Color::Cyan,
                        _    => Color::White
                    }
                } else {
                    Color::White
                };

            if decoration.is_none() {
                // Try to find a decoration somewhere in the sequence
                decoration = code.split(';')
                                 .flat_map(LsColors::parse_decoration)
                                 .next();
            }

            let ansi_style = decoration.unwrap_or(Color::normal)(color);

            Some(ansi_style)
        } else {
            None
        }
    }

    /// Add a new `LS_COLORS` entry.
    fn add_entry(&mut self, input: &str) {
        let mut parts = input.trim().split('=');
        if let Some(pattern) = parts.next() {
            if let Some(style_code) = parts.next() {
                // Ensure that the input was split into exactly two parts:
                if !parts.next().is_none() {
                    return;
                }

                if let Some(style) = LsColors::parse_style(style_code) {
                    // Try to match against one of the known codes
                    let res = LS_CODES.iter().find(|&&c| c == pattern);

                    if let Some(code) = res {
                        match code.as_ref() {
                            "di" => self.directory = style,
                            "ln" => self.symlink = style,
                            "ex" => self.executable = style,
                            _ => return
                        }
                    } else if pattern.starts_with("*.") {
                        let extension = String::from(pattern).split_off(2);
                        self.extensions.insert(extension, style);
                    }
                    else if pattern.starts_with('*') {
                        let filename = String::from(pattern).split_off(1);
                        self.filenames.insert(filename, style);
                    } else {
                        // Unknown/corrupt pattern
                        return;
                    }
                }
            }
        }
    }

    /// Generate a `LsColors` structure from a string.
    pub fn from_string(input: &str) -> LsColors {
        let mut lscolors = LsColors::default();

        for s in input.split(':') {
            lscolors.add_entry(s);
        }

        lscolors
    }

    pub fn print_with_style<'a>(&self, s: &str, style: PaintStyle<'a>) -> io::Result<()> {
        let style = match style {
            PaintStyle::Directory    => Some(Cow::Borrowed(&self.directory)),
            PaintStyle::Executable   => Some(Cow::Borrowed(&self.executable)),
            PaintStyle::Symlink      => Some(Cow::Borrowed(&self.symlink)),

            PaintStyle::Filename(f)  => {
                f.file_name()
                    .and_then(|n| n.to_str())
                    .and_then(|n| self.filenames.get(n))
                    .map(Cow::Borrowed)
                    .or_else(|| {
                        f.extension()
                            .and_then(|e| e.to_str())
                            .and_then(|e| self.extensions.get(e))
                            .map(Cow::Borrowed)
                    })
            }
        };

        if let Some(style) = style {
            let mut stdout = StandardStream::stdout(ColorChoice::Always);
            try!(stdout.set_color(&style.to_color_spec()));
            write!(&mut stdout, "{}", s)
        } else {
            write!(&mut io::stdout(), "{}", s)
        }
    }
}

#[derive(Copy, Clone)]
pub enum PaintStyle<'a> {
    Directory,
    Executable,
    Symlink,
    Filename(&'a Path),
}

#[derive(Debug, PartialEq, Clone)]
struct Style(Color, TextStyle);

impl Style {
    fn to_color_spec(&self) -> ColorSpec {
        let mut c = ColorSpec::new();

        c.set_fg(Some(self.0.clone()));

        match self.1 {
            TextStyle::Normal => {c.set_bold(false);},
            TextStyle::Bold   => {c.set_bold(true);},
            _                 => {},
        }

        c
    }
}

trait StyleColor {
    fn normal(self) -> Style;
    fn bold(self) -> Style;
    fn italic(self) -> Style;
    fn underline(self) -> Style;
}

impl StyleColor for Color {
    fn normal(self) -> Style {
        Style(self, TextStyle::Normal)
    }

    fn bold(self) -> Style {
        Style(self, TextStyle::Bold)
    }

    fn italic(self) -> Style {
        Style(self, TextStyle::Italic)
    }

    fn underline(self) -> Style {
        Style(self, TextStyle::Underline)
    }
}

#[derive(Debug, PartialEq, Clone)]
enum TextStyle {
    Normal,
    Bold,
    Italic,
    Underline,
}

#[test]
fn test_parse_simple() {
    assert_eq!(Some(Color::Red.normal()),
               LsColors::parse_style("31"));
}

#[test]
fn test_parse_decoration() {
    assert_eq!(Some(Color::Red.normal()),
               LsColors::parse_style("00;31"));

    assert_eq!(Some(Color::Blue.italic()),
               LsColors::parse_style("03;34"));

    assert_eq!(Some(Color::Cyan.bold()),
               LsColors::parse_style("01;36"));
}

#[test]
fn test_parse_decoration_backwards() {
    assert_eq!(Some(Color::Blue.italic()),
               LsColors::parse_style("34;03"));

    assert_eq!(Some(Color::Cyan.bold()),
               LsColors::parse_style("36;01"));

    assert_eq!(Some(Color::Red.normal()),
               LsColors::parse_style("31;00"));
}

// #[test]
// fn test_parse_256() {
//     assert_eq!(Some(Color::Fixed(115).normal()),
//                LsColors::parse_style("38;5;115"));

//     assert_eq!(Some(Color::Fixed(115).normal()),
//                LsColors::parse_style("00;38;5;115"));

//     assert_eq!(Some(Color::Fixed(119).bold()),
//                LsColors::parse_style("01;38;5;119"));

//     assert_eq!(Some(Color::Fixed(119).bold()),
//                LsColors::parse_style("38;5;119;01"));
// }

#[test]
fn test_from_string() {
    assert_eq!(LsColors::default(), LsColors::from_string(&String::new()));

    let result = LsColors::from_string(
        &String::from("rs=0:di=03;34:ln=01;36:*.foo=01;35:*README=33"));

    assert_eq!(Color::Blue.italic(), result.directory);
    assert_eq!(Color::Cyan.bold(), result.symlink);
    assert_eq!(Some(&Color::Magenta.bold()), result.extensions.get("foo"));
    assert_eq!(Some(&Color::Yellow.normal()), result.filenames.get("README"));
}
