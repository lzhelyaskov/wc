extern crate getopts;

use getopts::Options;
use std::{
    cmp,
    collections::HashMap,
    error::Error as _,
    fmt,
    fs::File,
    io::{self, Read, Write},
    path::Path,
};

pub type Format = fn(WordCountVec, Box<Write>) -> Result<(), io::Error>;
pub type SortBy = fn(&WordPair, &WordPair) -> cmp::Ordering;
pub type Dictionary = HashMap<String, u32>;
pub type WordPair = (String, u32);
pub type WordCountVec = Vec<WordPair>;
type ParseArgsResult =
    Result<(WordCountParams, Option<Vec<String>>, Option<String>, Format), ParamsError>;

#[derive(Debug)]
pub enum MyError {
    OpenFile(String, io::Error),
    ReadFile(String, io::Error),
    ReadDir(String, io::Error),
    ReadStdIn(io::Error),
    NotFileNorDir(String),
    InvalidPath(String),
}

impl fmt::Display for MyError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self {
            MyError::OpenFile(path, io_err) => {
                write!(f, "failed to open file: {}. io error: {}", path, io_err)
            }
            MyError::ReadFile(path, io_err) => {
                write!(f, "failed to read file: {}. io error: {}", path, io_err)
            }
            MyError::ReadDir(path, io_err) => write!(
                f,
                "failed to read directory: {}. io error: {}",
                path, io_err
            ),
            MyError::ReadStdIn(io_err) => {
                write!(f, "failed to read from stdin. io error: {}", io_err)
            }
            MyError::NotFileNorDir(path) => {
                write!(f, "path {} is not a directory nor a file", path)
            }
            MyError::InvalidPath(path) => write!(f, "invalid path: {}", path),
        }
    }
}

impl std::error::Error for MyError {}

#[derive(Debug)]
struct ParamsError {
    desc: String,
}

impl ParamsError {
    fn help(program: String, options: Options) -> Self {
        ParamsError {
            desc: {
                let brief = format!("Usage: {} [options]", program);
                let s = options.usage(&brief);
                format!("{}", s)
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
pub struct WordCountParams {
    /// disables case sensitivity
    /// if true "Example" and "eXample" are counted as the same word.
    ignore_case: bool,
    /// allows descent into subfolders in given path
    recursive: bool,
    /// controls if and how output should be sorted
    sort_by: Option<SortBy>,
}

impl Default for WordCountParams {
    fn default() -> Self {
        WordCountParams {
            ignore_case: false,
            recursive: false,
            sort_by: None,
        }
    }
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
            params: params,
            buf: String::new(),
            map: HashMap::new(),
        }
    }

    /// counts words in a file provided by path
    fn from_file(&mut self, path: &Path) -> Result<(), MyError> {
        File::open(path)
            .and_then(|mut file| file.read_to_string(&mut self.buf))
            .and_then(|_| {
                count_words(&self.buf, &mut self.map, self.params.ignore_case);
                self.buf.clear();
                Ok(())
            })
            .map_err(|res| MyError::ReadFile(path.display().to_string(), res))
    }

    /// counts words in files and / or in a directory provided by path
    fn from_dir(&mut self, path: &Path) -> Result<(), MyError> {
        path.read_dir()
            .map_err(|err| MyError::ReadDir(path.display().to_string(), err))
            .and_then(|mut read_dir| {
                let iter = read_dir.try_for_each(|entry_path| {
                    entry_path
                        .map_err(|err| MyError::ReadFile(path.display().to_string(), err))
                        .and_then(|entry_path| {
                            let path = entry_path.path();
                            if path.is_dir() && self.params.recursive {
                                self.from_dir(&path)
                            } else {
                                self.from_file(&path)
                            }
                        })
                });
                iter
            })
            .and_then(|_| Ok(()))
    }

    /// counts words read from stdin
    fn from_stdin(&mut self) -> Result<(), MyError> {
        let stdin = io::stdin();
        let mut handle = stdin.lock();

        handle
            .read_to_string(&mut self.buf)
            .and_then(|_| {
                count_words(&self.buf, &mut self.map, self.params.ignore_case);
                self.buf.clear();
                Ok(())
            })
            .map_err(|err| MyError::ReadStdIn(err))
    }

    /// collects word counts and returns them as vector of touples where
    /// the first value is a String representation of the word and
    /// the second value is a u32 number of occurances
    pub fn collect(mut self, in_path: Option<Vec<String>>) -> Result<WordCountVec, MyError> {
        match in_path {
            None => {
                if let Err(err) = self.from_stdin() {
                    return Err(err);
                }
            }
            Some(paths) => {
                for src in &paths {
                    let path = Path::new(&src);
                    if let Err(e) = {
                        if !path.exists() {
                            Err(MyError::InvalidPath(src.to_string()))
                        } else if dbg!(path.is_file()) {
                            self.from_file(path)
                        } else if dbg!(path.is_dir()) {
                            self.from_dir(path)
                        } else {
                            Err(MyError::NotFileNorDir(src.to_string()))
                        }
                    } {
                        return Err(e);
                    }
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

    let writer: Box<Write> = {
        if let Some(path) = out_path {
            match File::create(&path) {
                Ok(file) => Box::new(file),
                Err(error) => {
                    eprintln!(
                        "failed to create file: {}. error: {}",
                        &path,
                        error.description()
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
fn plain(items: WordCountVec, mut writer: Box<Write>) -> Result<(), io::Error> {
    for (key, value) in items {
        write!(*writer, "{} {}\n", key, value)?;
    }
    Ok(())
}

/// writes items formatted as csv
fn csv(items: WordCountVec, mut writer: Box<Write>) -> Result<(), io::Error> {
    write!(*writer, "word, count\n")?;
    for (key, value) in items {
        write!(*writer, "\"{}\", {}\n", key, value)?;
    }
    Ok(())
}

/// writes items as json
fn json(items: WordCountVec, mut writer: Box<Write>) -> Result<(), io::Error> {
    write!(*writer, "{{\n\t\"wordCount\": [\n")?;
    for (key, value) in items {
        write!(*writer, "\t\t\"{}\": {},\n", key, value)?;
    }
    write!(*writer, "\t]\n}}")
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

    let sort_by_fn: Option<SortBy> = match matches.opt_str("s") {
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
            ignore_case: ignore_case,
            recursive: recursive,
            sort_by: sort_by_fn,
        },
        in_path,
        out_path,
        out_format,
    ))
}

pub fn count_words(s: &str, map: &mut Dictionary, ignore_case: bool) {
    let words = s.split(|c: char| !c.is_alphabetic());
    let case: &dyn Fn(&str) -> String = if dbg!(ignore_case) {
        &|s: &str| s.to_lowercase().to_string()
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
