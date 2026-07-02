//! Built-in knowledge: the Python standard-library top-level module set, and a
//! curated import-name → distribution-name alias table (the `cv2`→`opencv-python`
//! long tail). A maintained alias table is a durable moat (RESEARCH.md §3.5).

use rustc_hash::{FxHashMap, FxHashSet};

/// A pragmatic subset of CPython 3.12 top-level stdlib modules. Not exhaustive;
/// extend over time. Used to exclude stdlib imports from "missing dependency".
const STDLIB: &[&str] = &[
    "__future__",
    "abc",
    "argparse",
    "array",
    "ast",
    "asyncio",
    "base64",
    "bisect",
    "builtins",
    "bz2",
    "calendar",
    "collections",
    "concurrent",
    "configparser",
    "contextlib",
    "contextvars",
    "copy",
    "csv",
    "ctypes",
    "dataclasses",
    "datetime",
    "decimal",
    "difflib",
    "dis",
    "email",
    "enum",
    "errno",
    "faulthandler",
    "fcntl",
    "filecmp",
    "fileinput",
    "fnmatch",
    "fractions",
    "functools",
    "gc",
    "getpass",
    "gettext",
    "glob",
    "graphlib",
    "gzip",
    "hashlib",
    "heapq",
    "hmac",
    "html",
    "http",
    "imaplib",
    "importlib",
    "inspect",
    "io",
    "ipaddress",
    "itertools",
    "json",
    "keyword",
    "logging",
    "lzma",
    "math",
    "mimetypes",
    "multiprocessing",
    "numbers",
    "operator",
    "os",
    "pathlib",
    "pickle",
    "pkgutil",
    "platform",
    "plistlib",
    "pprint",
    "profile",
    "pstats",
    "queue",
    "random",
    "re",
    "reprlib",
    "secrets",
    "select",
    "selectors",
    "shelve",
    "shlex",
    "shutil",
    "signal",
    "site",
    "smtplib",
    "socket",
    "socketserver",
    "sqlite3",
    "ssl",
    "stat",
    "statistics",
    "string",
    "struct",
    "subprocess",
    "sys",
    "sysconfig",
    "tarfile",
    "tempfile",
    "textwrap",
    "threading",
    "time",
    "timeit",
    "tkinter",
    "token",
    "tokenize",
    "traceback",
    "tracemalloc",
    "types",
    "typing",
    "unittest",
    "urllib",
    "uuid",
    "venv",
    "warnings",
    "wave",
    "weakref",
    "webbrowser",
    "xml",
    "xmlrpc",
    "zipfile",
    "zipimport",
    "zlib",
    "zoneinfo",
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
    ("jwt", "pyjwt"),
    ("MySQLdb", "mysqlclient"),
    ("psycopg2", "psycopg2-binary"),
    ("docx", "python-docx"),
    ("pptx", "python-pptx"),
];

/// Namespace-package top levels claimed by many unrelated distributions
/// (`google` alone is protobuf, google-api-python-client, google-cloud-*…).
/// Without an installed environment the owning distribution is unknowable,
/// so `missing-dependency` must not guess a name for these.
const NAMESPACE_TOPS: &[&str] = &["google", "azure", "backports", "zope"];

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

    /// Every distribution name that plausibly provides `module` (a dotted
    /// import path). First entry is the preferred name for messages. An
    /// import counts as "declared" if ANY candidate is declared: `psycopg2`
    /// and `psycopg2-binary` are both real dists providing `import psycopg2`,
    /// and namespace imports like `google.cloud.storage` are provided by the
    /// dashed dist (`google-cloud-storage`), not the bare top level.
    pub fn dists_for_import(&self, module: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut push = |d: String| {
            if !out.contains(&d) {
                out.push(d);
            }
        };
        let top = module.split('.').next().unwrap_or(module);
        push(self.dist_for_import(top));
        push(normalize_dist(top));
        // Dotted prefixes as dashed dist names (2–3 segments): covers
        // namespace packages (google-cloud-storage) and dotted dists
        // (ruamel-yaml).
        let segs: Vec<&str> = module.split('.').collect();
        for n in [2usize, 3] {
            if segs.len() >= n {
                push(normalize_dist(&segs[..n].join("-")));
            }
        }
        out
    }

    /// True if `top_level` is a namespace package claimed by many unrelated
    /// distributions — `missing-dependency` must not guess a name for it.
    pub fn is_namespace_top(&self, top_level: &str) -> bool {
        NAMESPACE_TOPS.contains(&top_level)
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

    #[test]
    fn import_candidates_cover_alias_and_bare_names() {
        let k = Known::new();
        // Both `psycopg2` and `psycopg2-binary` are real dists providing
        // `import psycopg2`; declaring either must count.
        let c = k.dists_for_import("psycopg2");
        assert!(c.contains(&"psycopg2-binary".to_string()));
        assert!(c.contains(&"psycopg2".to_string()));
        // Namespace imports: the dashed dist name is a candidate.
        let g = k.dists_for_import("google.cloud.storage");
        assert!(g.contains(&"google-cloud-storage".to_string()));
        assert!(k.is_namespace_top("google"));
        assert!(!k.is_namespace_top("requests"));
        // ruamel.yaml → ruamel-yaml.
        let r = k.dists_for_import("ruamel.yaml");
        assert!(r.contains(&"ruamel-yaml".to_string()));
    }
}
