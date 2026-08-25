mod list;
mod scan;
mod test_case;
mod workloads_toml;

pub use list::write_nextest_terse_list;
pub use scan::{filter_tests, scan_elf_files, scan_elf_files_by_stems};
pub use test_case::ElfTestCase;
pub use workloads_toml::{load_workload_spec, WorkloadTomlError};

use std::path::{Path, PathBuf};
use std::{collections::BTreeSet, fs};

use super::args::RegressionArgs;

pub fn discover_tests(
    args: &RegressionArgs,
    extension: Option<&str>,
    match_case: impl Fn(&ElfTestCase) -> bool,
) -> Result<Vec<ElfTestCase>, DiscoveryError> {
    // Priority: workload_toml > case_list > scan all
    let test_cases = if let Some(toml_path) = args.workload_toml().as_ref() {
        discover_from_workload_toml(args, extension, toml_path)?
    } else if args.case_list.is_some() {
        let bb_tests_root = args.bb_tests_root();
        if !bb_tests_root.exists() {
            return Err(DiscoveryError::RootMissing { path: bb_tests_root });
        }
        let test_cases = scan_elf_files(&bb_tests_root, extension);
        let test_cases = filter_tests(test_cases, args.filter.as_deref(), match_case);
        apply_case_list(test_cases, args)?
    } else {
        let bb_tests_root = args.bb_tests_root();
        if !bb_tests_root.exists() {
            return Err(DiscoveryError::RootMissing { path: bb_tests_root });
        }
        let test_cases = scan_elf_files(&bb_tests_root, extension);
        filter_tests(test_cases, args.filter.as_deref(), match_case)
    };

    Ok(test_cases)
}

fn discover_from_workload_toml(
    args: &RegressionArgs,
    extension: Option<&str>,
    toml_path: &Path,
) -> Result<Vec<ElfTestCase>, DiscoveryError> {
    let spec = load_workload_spec(toml_path).map_err(|e| DiscoveryError::WorkloadToml {
        path: toml_path.to_path_buf(),
        source: Box::new(e),
    })?;

    // Resolve search_path: if specified in toml, use it relative to bb_tests_root;
    // otherwise use bb_tests_root directly.
    let bb_tests_root = args.bb_tests_root();
    let search_root = if let Some(rel_path) = spec.search_path {
        bb_tests_root.join(rel_path)
    } else {
        bb_tests_root
    };

    if !search_root.exists() {
        return Err(DiscoveryError::RootMissing { path: search_root });
    }

    let (test_cases, missing, duplicates) = scan_elf_files_by_stems(&search_root, extension, &spec.tests);

    if !missing.is_empty() {
        return Err(DiscoveryError::WorkloadTomlMissing {
            path: toml_path.to_path_buf(),
            missing,
        });
    }
    if !duplicates.is_empty() {
        return Err(DiscoveryError::WorkloadTomlDuplicate {
            path: toml_path.to_path_buf(),
            duplicates,
        });
    }

    Ok(test_cases)
}

fn apply_case_list(tests: Vec<ElfTestCase>, args: &RegressionArgs) -> Result<Vec<ElfTestCase>, DiscoveryError> {
    let Some(path) = &args.case_list else {
        return Ok(tests);
    };

    let content = fs::read_to_string(path).map_err(|source| DiscoveryError::CaseListRead {
        path: path.clone(),
        source,
    })?;
    let requested: BTreeSet<String> = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| line.to_string())
        .collect();
    if requested.is_empty() {
        return Err(DiscoveryError::CaseListEmpty { path: path.clone() });
    }

    let mut selected = Vec::new();
    let mut found = BTreeSet::new();
    for tc in tests {
        if requested.contains(&tc.stem) || requested.contains(&tc.name) {
            found.insert(tc.stem.clone());
            found.insert(tc.name.clone());
            selected.push(tc);
        }
    }

    let missing: Vec<String> = requested.into_iter().filter(|name| !found.contains(name)).collect();
    if !missing.is_empty() {
        return Err(DiscoveryError::CaseListMissing {
            path: path.clone(),
            missing,
        });
    }

    Ok(selected)
}

#[derive(Debug)]
pub enum DiscoveryError {
    RootMissing {
        path: PathBuf,
    },
    CaseListRead {
        path: PathBuf,
        source: std::io::Error,
    },
    CaseListEmpty {
        path: PathBuf,
    },
    CaseListMissing {
        path: PathBuf,
        missing: Vec<String>,
    },
    WorkloadToml {
        path: PathBuf,
        source: Box<WorkloadTomlError>,
    },
    WorkloadTomlMissing {
        path: PathBuf,
        missing: Vec<String>,
    },
    WorkloadTomlDuplicate {
        path: PathBuf,
        duplicates: Vec<(String, Vec<PathBuf>)>,
    },
}

impl std::fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiscoveryError::RootMissing { path } => {
                write!(f, "Search root directory not found: {}", path.display())
            }
            DiscoveryError::CaseListRead { path, source } => {
                write!(f, "Failed to read case list {}: {}", path.display(), source)
            }
            DiscoveryError::CaseListEmpty { path } => {
                write!(f, "Case list is empty: {}", path.display())
            }
            DiscoveryError::CaseListMissing { path, missing } => {
                write!(
                    f,
                    "Case list {} has unknown test case(s): {}",
                    path.display(),
                    missing.join(", ")
                )
            }
            DiscoveryError::WorkloadToml { path, source } => {
                write!(f, "Workload TOML error for {}: {}", path.display(), source)
            }
            DiscoveryError::WorkloadTomlMissing { path, missing } => {
                write!(
                    f,
                    "Workload TOML {} has unknown test case(s): {}",
                    path.display(),
                    missing.join(", ")
                )
            }
            DiscoveryError::WorkloadTomlDuplicate { path, duplicates } => {
                let details = duplicates
                    .iter()
                    .map(|(stem, paths)| {
                        let paths = paths
                            .iter()
                            .map(|path| path.display().to_string())
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("{stem}: {paths}")
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                write!(
                    f,
                    "Workload TOML {} has duplicate test case(s): {}",
                    path.display(),
                    details
                )
            }
        }
    }
}

impl std::error::Error for DiscoveryError {}
