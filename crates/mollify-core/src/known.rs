//! Built-in knowledge: the Python standard-library top-level module set, and a
//! curated import-name → distribution-name alias table (the `cv2`→`opencv-python`
//! long tail). A maintained alias table is a durable moat (RESEARCH.md §3.5).

use rustc_hash::{FxHashMap, FxHashSet};

/// A pragmatic subset of CPython 3.12 top-level stdlib modules. Not exhaustive;
/// extend over time. Used to exclude stdlib imports from "missing dependency".
const STDLIB: &[&str] = &[
    "__future__", "abc", "argparse", "array", "ast", "asyncio", "base64", "bisect",
    "builtins", "bz2", "calendar", "collections", "concurrent", "configparser",
    "contextlib", "contextvars", "copy", "csv", "ctypes", "dataclasses", "datetime",
    "decimal", "difflib", "dis", "email", "enum", "errno", "faulthandler", "fcntl",
    "filecmp", "fileinput", "fnmatch", "fractions", "functools", "gc", "getpass",
    "gettext", "glob", "graphlib", "gzip", "hashlib", "heapq", "hmac", "html", "http",
    "imaplib", "importlib", "inspect", "io", "ipaddress", "itertools", "json", "keyword",
    "logging", "lzma", "math", "mimetypes", "multiprocessing", "numbers", "operator",
    "os", "pathlib", "pickle", "pkgutil", "platform", "plistlib", "pprint", "profile",
    "pstats", "queue", "random", "re", "reprlib", "secrets", "select", "selectors",
    "shelve", "shlex", "shutil", "signal", "site", "smtplib", "socket", "socketserver",
    "sqlite3", "ssl", "stat", "statistics", "string", "struct", "subprocess", "sys",
    "sysconfig", "tarfile", "tempfile", "textwrap", "threading", "time", "timeit",
    "tkinter", "token", "tokenize", "traceback", "tracemalloc", "types", "typing",
    "unittest", "urllib", "uuid", "venv", "warnings", "wave", "weakref", "webbrowser",
    "xml", "xmlrpc", "zipfile", "zipimport", "zlib", "zoneinfo",
];

/// import-name → distribution-name (PyPI). Reverse-used to decide whether a
/// declared dependency is actually imported.
const ALIASES: &[(&str, &str)] = &[
    ("cv2", "opencv-python"),
    ("PIL", "pillow"),
    ("yaml", "pyyaml"),
    ("sklearn", "scikit-learn"),
    ("bs4", "beautifulsoup4"),
    ("dateutil", "python-dateutil"),
    ("dotenv", "python-dotenv"),
    ("jose", "python-jose"),
    ("attr", "attrs"),
    ("git", "gitpython"),
    ("OpenSSL", "pyopenssl"),
    ("serial", "pyserial"),
    ("Crypto", "pycryptodome"),
    ("google", "google-api-python-client"),
    ("jwt", "pyjwt"),
    ("MySQLdb", "mysqlclient"),
    ("psycopg2", "psycopg2-binary"),
    ("docx", "python-docx"),
    ("pptx", "python-pptx"),
    ("markdown", "markdown"),
];

pub struct Known {
    stdlib: FxHashSet<&'static str>,
    /// import-name -> normalized distribution-name.
    alias: FxHashMap<&'static str, &'static str>,
}

impl Known {
    pub fn new() -> Self {
        Known {
            stdlib: STDLIB.iter().copied().collect(),
            alias: ALIASES.iter().copied().collect(),
        }
    }

    pub fn is_stdlib(&self, top_level: &str) -> bool {
        self.stdlib.contains(top_level)
    }

    /// The normalized distribution name an import maps to. PEP 503 normalization
    /// (lowercase, runs of `-_.` → `-`) plus the alias table.
    pub fn dist_for_import(&self, top_level: &str) -> String {
        if let Some(d) = self.alias.get(top_level) {
            return normalize_dist(d);
        }
        normalize_dist(top_level)
    }
}

impl Default for Known {
    fn default() -> Self {
        Self::new()
    }
}

/// PEP 503 name normalization.
pub fn normalize_dist(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut prev_sep = false;
    for c in name.to_ascii_lowercase().chars() {
        if c == '-' || c == '_' || c == '.' {
            if !prev_sep {
                out.push('-');
                prev_sep = true;
            }
        } else {
            out.push(c);
            prev_sep = false;
        }
    }
    out.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_is_pep503() {
        assert_eq!(normalize_dist("Flask_SQLAlchemy"), "flask-sqlalchemy");
        assert_eq!(normalize_dist("scikit.learn"), "scikit-learn");
    }

    #[test]
    fn aliases_and_stdlib() {
        let k = Known::new();
        assert!(k.is_stdlib("os"));
        assert!(!k.is_stdlib("numpy"));
        assert_eq!(k.dist_for_import("cv2"), "opencv-python");
        assert_eq!(k.dist_for_import("requests"), "requests");
    }
}
