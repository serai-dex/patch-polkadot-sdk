use std::{
  collections::HashSet,
  fs,
  path::{Path, PathBuf},
  env,
};

struct Crate {
  path: PathBuf,
  name: String,
}

fn discover_all_crates_in_folder(folder: impl AsRef<Path>) -> Vec<Crate> {
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

fn remove_folder_of_crates(folder: &str) {
  // Discover the crates we're removing
  let removed_crates = discover_all_crates_in_folder(folder);
  if removed_crates.is_empty() {
    return;
  }
  let mut removed_crates_names =
    removed_crates.iter().map(|removed| removed.name.as_str().to_owned()).collect::<HashSet<_>>();

  // Remove them
  assert!(fs::canonicalize(folder).unwrap().starts_with(env::current_dir().unwrap()));
  fs::remove_dir_all(folder).unwrap();

  // Remove the crates from the root-level `Cargo.toml`
  let root_level_toml_path = "Cargo.toml";
  let root_level_toml = fs::read_to_string(root_level_toml_path).unwrap();
  let mut root_level_toml = toml::from_str::<toml::Table>(&root_level_toml).unwrap();
  let mut remove_from_members = |members| {
    let Some(members) = root_level_toml["workspace"].get_mut(members) else { return };
    let members = members.as_array_mut().unwrap();
    let mut i = 0;
    'member: while i < members.len() {
      if members[i].as_str().unwrap().starts_with(folder) {
        members.remove(i);
        continue 'member;
      }
      i += 1;
    }
  };
  remove_from_members("members");
  remove_from_members("default-members");
  if let Some(default_members) = root_level_toml["workspace"].get("default-members") {
    if default_members.as_array().unwrap().is_empty() {
      root_level_toml["workspace"].as_table_mut().unwrap().remove("default-members");
    }
  }
  {
    let dependencies = root_level_toml["workspace"]["dependencies"].as_table_mut().unwrap();
    for removed_crate in &removed_crates_names {
      dependencies.remove(removed_crate);
    }
    let remaining = dependencies.keys().map(|key| key.as_str().to_owned()).collect::<Vec<_>>();
    for remaining in remaining {
      if let Some(table) = dependencies[&remaining].as_table() {
        if let Some(package) = table.get("package") {
          let package = package.as_str().unwrap();
          if removed_crates_names.contains(package) {
            dependencies.remove(&remaining);
            removed_crates_names.insert(remaining);
          }
        }
      }
    }
  }
  fs::write(root_level_toml_path, toml::to_string_pretty(&root_level_toml).unwrap().as_bytes())
    .unwrap();

  // Discover the remaining crates in the repository
  let remaining_crates = discover_all_crates_in_folder(".");
  // Remove all dependencies on now-deleted crates
  for item in remaining_crates {
    let crate_cargo_toml_path = item.path.join("Cargo.toml");
    let crate_cargo_toml = fs::read_to_string(&crate_cargo_toml_path).unwrap();
    let mut crate_cargo_toml = toml::from_str::<toml::Table>(&crate_cargo_toml).unwrap();
    for (key, value) in crate_cargo_toml.iter_mut() {
      if !HashSet::from(["dependencies", "build-dependencies", "dev-dependencies"])
        .contains(key.as_str())
      {
        continue;
      }

      let toml::Value::Table(table) = value else {
        panic!("table wasn't a table");
      };
      for removed_crate in &removed_crates_names {
        table.remove(removed_crate);
      }
      let remaining = table.keys().map(|key| key.as_str().to_string()).collect::<Vec<_>>();
      for remaining in remaining {
        if let Some(table) = table[&remaining].as_table_mut() {
          if let Some(package) = table.get("package") {
            let package = package.as_str().unwrap();
            if removed_crates_names.contains(package) {
              table.remove(&remaining);
            }
          }
        }
      }
    }
    if let Some(features) = crate_cargo_toml.get_mut("features") {
      let features = features.as_table_mut().unwrap();
      for (_key, value) in features {
        let value = value.as_array_mut().unwrap();
        let mut i = 0;
        while i < value.len() {
          if removed_crates_names.contains(value[i].as_str().unwrap().split("/").next().unwrap()) {
            value.remove(i);
            continue;
          }
          i += 1;
        }
      }
    }
    fs::write(crate_cargo_toml_path, toml::to_string_pretty(&crate_cargo_toml).unwrap().as_bytes())
      .unwrap();
  }
}

fn main() {
  env::set_current_dir("polkadot-sdk").unwrap();

  let mut args = env::args();
  let _program = args.next().unwrap();

  match args.next().unwrap().as_str() {
    "remove_crate_tree" => {
      remove_folder_of_crates(&args.next().unwrap());
    }
    "remove_dependency" => {
      let member = args.next().unwrap();

      let root_level_toml_path = "Cargo.toml";
      let root_level_toml = fs::read_to_string(root_level_toml_path).unwrap();
      let mut root_level_toml = toml::from_str::<toml::Table>(&root_level_toml).unwrap();
      {
        let dependencies = root_level_toml["workspace"]["dependencies"].as_table_mut().unwrap();
        dependencies.remove(&member);
        let remaining = dependencies.keys().map(|key| key.as_str().to_string()).collect::<Vec<_>>();
        for remaining in remaining {
          if let Some(table) = dependencies[&remaining].as_table() {
            if let Some(package) = table.get("package") {
              let package = package.as_str().unwrap();
              if package == member {
                dependencies.remove(&remaining);
              }
            }
          }
        }
      }
      fs::write(root_level_toml_path, toml::to_string_pretty(&root_level_toml).unwrap().as_bytes())
        .unwrap();
    }
    "remove_feature" => {
      let feature = args.next().unwrap();
      for item in discover_all_crates_in_folder(".") {
        let crate_cargo_toml_path = item.path.join("Cargo.toml");
        let crate_cargo_toml = fs::read_to_string(&crate_cargo_toml_path).unwrap();
        let mut crate_cargo_toml = toml::from_str::<toml::Table>(&crate_cargo_toml).unwrap();
        for dependencies in ["dependencies", "build-dependencies"] {
          if let Some(dependencies) = crate_cargo_toml.get_mut(dependencies) {
            let dependencies = dependencies.as_table_mut().unwrap();
            for (_dependency, value) in dependencies {
              if let Some(value) = value.as_table_mut() {
                if let Some(value) = value.get_mut("features") {
                  let value = value.as_array_mut().unwrap();
                  let mut i = 0;
                  while i < value.len() {
                    if value[i].as_str().unwrap() == feature {
                      value.remove(i);
                      continue;
                    }
                    i += 1;
                  }
                }
              }
            }
          }
        }
        if let Some(features) = crate_cargo_toml.get_mut("features") {
          let features = features.as_table_mut().unwrap();
          features.remove(&feature);
          for (_name, value) in features {
            let value = value.as_array_mut().unwrap();
            let mut i = 0;
            while i < value.len() {
              if value[i].as_str().unwrap().ends_with(&format!("/{feature}")) {
                value.remove(i);
                continue;
              }
              i += 1;
            }
          }
        }
        fs::write(
          crate_cargo_toml_path,
          toml::to_string_pretty(&crate_cargo_toml).unwrap().as_bytes(),
        )
        .unwrap();
      }
    }
    "remove_dev_dependencies" => {
      for item in discover_all_crates_in_folder(".") {
        // If this path still exists, even after the existing removals of test code...
        if !fs::exists(&item.path).unwrap() {
          continue;
        }
        // Remove its test code, if it has any
        remove_folder_of_crates(
          &fs::canonicalize(&item.path)
            .unwrap()
            .strip_prefix(env::current_dir().unwrap())
            .unwrap()
            .join("test")
            .display()
            .to_string(),
        );
        remove_folder_of_crates(
          &fs::canonicalize(&item.path)
            .unwrap()
            .strip_prefix(env::current_dir().unwrap())
            .unwrap()
            .join("benches")
            .display()
            .to_string(),
        );

        let crate_cargo_toml_path = item.path.join("Cargo.toml");
        let crate_cargo_toml = fs::read_to_string(&crate_cargo_toml_path).unwrap();
        let mut crate_cargo_toml = toml::from_str::<toml::Table>(&crate_cargo_toml).unwrap();
        if let Some(removed) = crate_cargo_toml.remove("dev-dependencies") {
          let mut removed = removed
            .as_table()
            .unwrap()
            .keys()
            .map(|key| key.as_str().to_string())
            .collect::<HashSet<_>>();
          for depend in crate_cargo_toml
            .get("dependencies")
            .map(|dependencies| dependencies.as_table().unwrap().keys())
            .into_iter()
            .flatten()
          {
            removed.remove(depend);
          }
          for depend in crate_cargo_toml
            .get("build-dependencies")
            .map(|dependencies| dependencies.as_table().unwrap().keys())
            .into_iter()
            .flatten()
          {
            removed.remove(depend);
          }
          let unique_dev_dependencies = removed;

          if let Some(features) = crate_cargo_toml.get_mut("features") {
            for (_feature, value) in features.as_table_mut().unwrap().iter_mut() {
              let value = value.as_array_mut().unwrap();
              let mut i = 0;
              while i < value.len() {
                if unique_dev_dependencies
                  .contains(value[i].as_str().unwrap().split('/').next().unwrap())
                {
                  value.remove(i);
                  continue;
                }
                i += 1;
              }
            }
          }
        }
        fs::write(
          crate_cargo_toml_path,
          toml::to_string_pretty(&crate_cargo_toml).unwrap().as_bytes(),
        )
        .unwrap();
      }
    }
    _ => panic!("unknown command"),
  }
}
