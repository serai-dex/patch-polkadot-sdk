use std::{fs, path::Path};

use crate::Crate;

pub(crate) fn discover_all_crates_in_folder(folder: impl AsRef<Path>) -> Vec<Crate> {
  if !fs::exists(&folder).unwrap() {
    return vec![];
  }

  let mut folders = vec![folder.as_ref().to_owned()];
  let mut crates = vec![];
  while let Some(folder) = folders.pop() {
    for entry in fs::read_dir(&folder).unwrap() {
      let entry = entry.unwrap();
      if entry.file_name() == "Cargo.toml" {
        for line in fs::read_to_string(entry.path()).unwrap().lines() {
          if let Some(name) = line.strip_prefix("name = \"") {
            let mut name = name.to_owned();
            assert_eq!(name.pop(), Some('"'));
            crates.push(Crate { path: folder.to_owned(), name });
          }
        }
      }
      if entry.file_type().unwrap().is_dir() {
        folders.push(entry.path());
      }
    }
  }

  crates
}
