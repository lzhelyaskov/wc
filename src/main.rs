extern crate getopts;

use getopts::Options;
use std::{
    cmp,
    collections::HashMap,
    fmt,
    fs::File,
    io::{self, Read, Write},
    path::Path,
};

pub type Format = fn(WordCountVec, Box<dyn Write>) -> Result<(), io::Error>;
pub type SortBy = fn(&WordPair, &WordPair) -> cmp::Ordering;
pub type Dictionary = HashMap<String, u32>;
pub type WordPair = (String, u32);
pub type WordCountVec = Vec<WordPair>;
type ParseArgsResult =
    Result<(WordCountParams, Option<Vec<String>>, Option<String>, Format), ParamsError>;

#[derive(Debug)]
pub enum WcError {
    OpenFile(String, io::Error),
    ReadFile(String, io::Error),
    ReadDir(String, io::Error),
    ReadStdIn(io::Error),
    NotFileNorDir(String),
    InvalidPath(String),
}

impl fmt::Display for WcError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self {
            WcError::OpenFile(path, io_err) => {
                write!(f, "failed to open file: {}. io error: {}", path, io_err)
            }
            WcError::ReadFile(path, io_err) => {
                write!(f, "failed to read file: {}. io error: {}", path, io_err)
            }
            WcError::ReadDir(path, io_err) => write!(
                f,
                "failed to read directory: {}. io error: {}",
                path, io_err
            ),
            WcError::ReadStdIn(io_err) => {
                write!(f, "failed to read from stdin. io error: {}", io_err)
            }
            WcError::NotFileNorDir(path) => {
                write!(f, "path {} is not a directory nor a file", path)
            }
            WcError::InvalidPath(path) => write!(f, "invalid path: {}", path),
        }
    }
}

impl std::error::Error for WcError {}

#[derive(Debug)]
struct ParamsError {
    desc: String,
}

impl ParamsError {
    fn help(program: String, options: Options) -> Self {
        ParamsError {
            desc: {
                let brief = format!("Usage: {} [options]", program);
                options.usage(&brief)
            },
        }
    }

    fn parse(fail: getopts::Fail) -> Self {
        ParamsError {
            desc: format!("failed to parse args. {}", fail),
        }
    }

    fn format(f: String) -> Self {
        ParamsError {
            desc: format!(
                "could not parse output format option {}. \nvalid values are 'json' or 'csv'",
                f
            ),
        }
    }

    fn sort(s: String) -> Self {
        ParamsError {
            desc: format!("could not parse 'sort by' option {}. \nvalid values are 'count', 'count-desc', 'alpha' and 'alpha-desc'", s),
        }
    }

    fn empty_path() -> Self {
        ParamsError {
            desc: "although option -p was provided, no actual path was given.".to_string(),
        }
    }
}

impl fmt::Display for ParamsError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.desc)
    }
}

impl std::error::Error for ParamsError {
    fn description(&self) -> &str {
        self.desc.as_ref()
    }
}

/// holds parameters for WordCount
#[derive(Default)]
pub struct WordCountParams {
    /// disables case sensitivity
    /// if true "Example" and "eXample" are counted as the same word.
    ignore_case: bool,
    /// allows descent into subfolders in given path
    recursive: bool,
    /// controls if and how output should be sorted
    sort_by: Option<SortBy>,
}

/**
 * todo
 */
pub struct WordCount {
    params: WordCountParams,
    buf: String,
    /// this map holds collected word counts <br>
    /// key: word.
    /// value: number of occurances of the word
    map: Dictionary,
}

impl WordCount {
    pub fn new(params: WordCountParams) -> Self {
        WordCount {
            params,
            buf: String::new(),
            map: HashMap::new(),
        }
    }

    /// counts words in a file provided by path
    fn read_file(&mut self, path: &Path) -> Result<(), WcError> {
        File::open(path)
            .and_then(|mut file| file.read_to_string(&mut self.buf))
            .map(|_| {
                count_words(&self.buf, &mut self.map, self.params.ignore_case);
                self.buf.clear();
            })
            .map_err(|res| WcError::ReadFile(path.display().to_string(), res))
    }

    /// counts words in files and / or in a directory provided by path
    fn read_dir(&mut self, path: &Path) -> Result<(), WcError> {
        path.read_dir()
            .map_err(|err| WcError::ReadDir(path.display().to_string(), err))
            .and_then(|mut read_dir| {
                read_dir.try_for_each(|entry_path| {
                    entry_path
                        .map_err(|err| WcError::ReadFile(path.display().to_string(), err))
                        .and_then(|entry_path| {
                            let path = entry_path.path();
                            if path.is_dir() && self.params.recursive {
                                self.read_dir(&path)
                            } else {
                                self.read_file(&path)
                            }
                        })
                })
            })
            .map(|_| ())
    }

    /// counts words read from stdin
    fn read_stdin(&mut self) -> Result<(), WcError> {
        let stdin = io::stdin();
        let mut handle = stdin.lock();

        handle
            .read_to_string(&mut self.buf)
            .map(|_| {
                count_words(&self.buf, &mut self.map, self.params.ignore_case);
                self.buf.clear();
            })
            .map_err(WcError::ReadStdIn)
    }

    /// collects word counts and returns them as vector of touples where
    /// the first value is a String representation of the word and
    /// the second value is a u32 number of occurances
    pub fn collect(mut self, in_path: Option<Vec<String>>) -> Result<WordCountVec, WcError> {
        match in_path {
            None => {
                self.read_stdin()?;
            }
            Some(paths) => {
                for src in &paths {
                    let path = Path::new(&src);
                    {
                        if !path.exists() {
                            Err(WcError::InvalidPath(src.to_string()))
                        } else if dbg!(path.is_file()) {
                            self.read_file(path)
                        } else if dbg!(path.is_dir()) {
                            self.read_dir(path)
                        } else {
                            Err(WcError::NotFileNorDir(src.to_string()))
                        }
                    }?
                }
            }
        }
        Ok(self.count())
    }

    /// flattens map to vec (sorted if sort_by provided)
    fn count(self) -> WordCountVec {
        let mut v: WordCountVec = self.map.into_iter().collect();
        if let Some(sort_by) = self.params.sort_by {
            v.sort_by(sort_by);
        }
        v
    }
}

fn main() {
    let (params, in_path, out_path, write) = {
        let args: Vec<String> = std::env::args().collect();
        match parse_args(args) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("{}", e);
                return;
            }
        }
    };

    let items = {
        match WordCount::new(params).collect(in_path) {
            Ok(items) => items,
            Err(e) => {
                eprintln!("{}", e);
                return;
            }
        }
    };

    let writer: Box<dyn Write> = {
        if let Some(path) = out_path {
            match File::create(&path) {
                Ok(file) => Box::new(file),
                Err(error) => {
                    eprintln!(
                        "failed to create file: {}. error: {}",
                        &path,
                        error
                    );
                    return;
                }
            }
        } else {
            Box::new(io::stdout())
        }
    };

    if let Err(error) = (write)(items, writer) {
        eprintln!("failed to write result.\n{:?}", error);
    }
}

/// writes items
fn plain(items: WordCountVec, mut writer: Box<dyn Write>) -> Result<(), io::Error> {
    for (key, value) in items {
        writeln!(*writer, "{} {}", key, value)?;
    }
    Ok(())
}

/// writes items formatted as csv
fn csv(items: WordCountVec, mut writer: Box<dyn Write>) -> Result<(), io::Error> {
    writeln!(*writer, "word, count")?;
    for (key, value) in items {
        writeln!(*writer, "\"{}\", {}", key, value)?;
    }
    Ok(())
}

/// writes items as json
fn json(items: WordCountVec, mut writer: Box<dyn Write>) -> Result<(), io::Error> {
    writeln!(*writer, "{{\n\t\"wordCount\": [")?;
    for (key, value) in items {
        writeln!(*writer, "\t\t\"{}\": {},", key, value)?;
    }
    writeln!(*writer, "\t]}}")
}

/// sort by word count ascending
fn count_asc(first: &WordPair, second: &WordPair) -> cmp::Ordering {
    let (first, a) = first;
    let (second, b) = second;
    let count = a.cmp(b);
    if count == cmp::Ordering::Equal {
        let len = first.len().cmp(&second.len());
        if len == cmp::Ordering::Equal {
            first.cmp(second)
        } else {
            len
        }
    } else {
        count
    }
}

/// sort by word count descending
fn count_desc(first: &WordPair, second: &WordPair) -> cmp::Ordering {
    let (first, a) = first;
    let (second, b) = second;
    let ord = b.cmp(a);
    if ord == cmp::Ordering::Equal {
        second.cmp(first)
    } else {
        ord
    }
}

/// sort alphabetically ascending
fn alpha_asc(first: &WordPair, second: &WordPair) -> cmp::Ordering {
    let (first, _) = first;
    let (second, _) = second;
    first.cmp(second)
}

/// sort alphabetically desending
fn alpha_desc(first: &WordPair, second: &WordPair) -> cmp::Ordering {
    let (first, _) = first;
    let (second, _) = second;
    second.cmp(first)
}

fn parse_args(args: Vec<String>) -> ParseArgsResult {
    let mut options = Options::new();
    options
        .optflag("h", "help", "print this help")
        .optflag("i", "ignore-case", "ignore case (not case sensitive)")
        .optflag("r", "recursive", "parse subfolders")
        .optopt(
            "p",
            "path",
            "sets desired path to a file or a folder to parse",
            "IN_PATH",
        )
        .optopt(
            "o",
            "output",
            "path and file name for output file",
            "OUT_PATH",
        )
        .optopt(
            "s",
            "sortby",
            "criteria to sort by",
            "[count|count-desc|alpha|alpha-desc]",
        )
        .optopt("f", "format", "output format", "[json|csv]");

    let matches = match options.parse(&args[1..]) {
        Ok(m) => m,
        Err(f) => return Err(ParamsError::parse(f)),
    };

    if matches.opt_present("h") {
        return Err(ParamsError::help(args[0].clone(), options));
    }

    let ignore_case = matches.opt_present("i");
    let recursive = matches.opt_present("r");
    let in_path = if matches.opt_present("p") {
        let t = matches.opt_strs("p");
        if t.is_empty() {  // this should never happen
            return Err(ParamsError::empty_path());
        }
        Some(t)
    } else {
        None
    };
    let out_path = matches.opt_str("o");
    let out_format = match matches.opt_str("f") {
        Some(s) => match s.to_lowercase().as_ref() {
            "json" => json,
            "csv" => csv,
            _ => return Err(ParamsError::format(s)),
        },
        None => plain,
    };

    let sort_by: Option<SortBy> = match matches.opt_str("s") {
        Some(s) => match s.to_lowercase().as_ref() {
            "count" => Some(count_asc),
            "count-desc" => Some(count_desc),
            "alpha" => Some(alpha_asc),
            "alpha-desc" => Some(alpha_desc),
            _ => return Err(ParamsError::sort(s)),
        },
        None => None,
    };

    Ok((
        WordCountParams {
            ignore_case,
            recursive,
            sort_by,
        },
        in_path,
        out_path,
        out_format,
    ))
}

pub fn count_words(s: &str, map: &mut Dictionary, ignore_case: bool) {
    let words = s.split(|c: char| !c.is_alphabetic());
    let case: &dyn Fn(&str) -> String = if dbg!(ignore_case) {
        &|s: &str| s.to_lowercase()
    } else {
        &|s: &str| s.to_string()
    };

    for word in words {
        if word.is_empty() {
            continue;
        }
        let w = case(word);
        let count = map.entry(w).or_insert(0);
        *count += 1;
    }
}
