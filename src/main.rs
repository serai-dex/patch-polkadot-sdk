use std::{
  collections::HashSet,
  fs,
  path::{Path, PathBuf},
  env,
};

mod discover;
use discover::discover_all_crates_in_folder;

mod crate_toml;
use crate_toml::{remove_dependencies_from_features, remove_dependencies};

mod workspace_toml;
use workspace_toml::workspace_toml;

#[derive(Clone)]
struct Crate {
  path: PathBuf,
  name: String,
}

fn safe_remove_dir(folder: impl AsRef<Path>) {
  // Ensure we're ONLY removing a folder within our subtree, and haven't escaped somehow
  assert!(fs::canonicalize(folder.as_ref()).unwrap().starts_with(env::current_dir().unwrap()));
  fs::remove_dir_all(folder).unwrap();
}

fn remove_from_array(array: &mut toml::value::Array, should_remove: impl Fn(&str) -> bool) {
  let mut i = 0;
  while i < array.len() {
    if should_remove(array[i].as_str().unwrap()) {
      array.remove(i);
      continue;
    }
    i += 1;
  }
}

fn remove_folder_of_crates(folder: &str) {
  if !fs::exists(folder).unwrap() {
    return;
  }

  // Discover the crates we're removing
  let removed_crates = discover_all_crates_in_folder(folder);
  // Remove them
  safe_remove_dir(folder);

  let mut removed_crates_names =
    removed_crates.iter().map(|removed| removed.name.as_str().to_owned()).collect::<HashSet<_>>();

  // Remove the crates from the root-level `Cargo.toml`
  let (workspace_toml_path, mut workspace_toml) = workspace_toml();
  let mut remove_from_members = |members| {
    let Some(members) = workspace_toml["workspace"].get_mut(members) else { return };
    let members = members.as_array_mut().unwrap();
    remove_from_array(members, |member| member.starts_with(folder));
  };
  remove_from_members("members");
  workspace_toml::remove_from_default_members(&mut workspace_toml, folder);
  {
    let dependencies = workspace_toml["workspace"]["dependencies"].as_table_mut().unwrap();
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
  fs::write(workspace_toml_path, toml::to_string_pretty(&workspace_toml).unwrap().as_bytes())
    .unwrap();

  // Discover the remaining crates in the repository
  let remaining_crates = discover_all_crates_in_folder(".");
  // Remove all dependencies on now-deleted crates
  for item in remaining_crates {
    let crate_toml_path = item.path.join("Cargo.toml");
    remove_dependencies(crate_toml_path, removed_crates_names.clone());
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

    "remove_workspace_dependency" => {
      let member = args.next().unwrap();

      let (workspace_toml_path, mut workspace_toml) = workspace_toml();
      {
        let dependencies = workspace_toml["workspace"]["dependencies"].as_table_mut().unwrap();

        // Remove it as a workspace dependency
        dependencies.remove(&member);

        // Check if it was aliased, and if so, remove anything which aliased it
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
      fs::write(workspace_toml_path, toml::to_string_pretty(&workspace_toml).unwrap().as_bytes())
        .unwrap();
    }

    "remove_feature" => {
      let feature = args.next().unwrap();
      for item in discover_all_crates_in_folder(".") {
        let crate_toml_path = item.path.join("Cargo.toml");
        let crate_toml = fs::read_to_string(&crate_toml_path).unwrap();
        let mut crate_toml = toml::from_str::<toml::Table>(&crate_toml).unwrap();

        // Remove all activations of this feature
        for dependencies in ["dependencies", "build-dependencies"] {
          if let Some(dependencies) = crate_toml.get_mut(dependencies) {
            let dependencies = dependencies.as_table_mut().unwrap();
            for (_dependency, value) in dependencies {
              if let Some(value) = value.as_table_mut() {
                if let Some(value) = value.get_mut("features") {
                  let value = value.as_array_mut().unwrap();
                  remove_from_array(value, |feature_name| feature_name == feature);
                }
              }
            }
          }
        }

        if let Some(features) = crate_toml.get_mut("features") {
          let features = features.as_table_mut().unwrap();

          // Remove the declaration of this feature
          features.remove(&feature);

          // Remove the conditional activations of this feature
          for (_name, value) in features {
            let value = value.as_array_mut().unwrap();
            remove_from_array(value, |feature_definition| {
              feature_definition.ends_with(&format!("/{feature}"))
            });
          }
        }

        fs::write(crate_toml_path, toml::to_string_pretty(&crate_toml).unwrap().as_bytes())
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
        let relative_path = fs::canonicalize(&item.path)
          .unwrap()
          .strip_prefix(env::current_dir().unwrap())
          .unwrap()
          .to_owned();
        remove_folder_of_crates(&relative_path.join("test").display().to_string());
        remove_folder_of_crates(&relative_path.join("benches").display().to_string());
        remove_folder_of_crates(&relative_path.join("examples").display().to_string());

        let crate_toml_path = item.path.join("Cargo.toml");
        let crate_toml = fs::read_to_string(&crate_toml_path).unwrap();
        let mut crate_toml = toml::from_str::<toml::Table>(&crate_toml).unwrap();

        // Remove all "dev-dependencies"
        if let Some(removed) = crate_toml.remove("dev-dependencies") {
          let dev_dependencies = removed
            .as_table()
            .unwrap()
            .keys()
            .map(|key| key.as_str().to_string())
            .collect::<HashSet<_>>();
          let dependency_names = |field| {
            crate_toml
              .get(field)
              .into_iter()
              .flat_map(|dependencies| dependencies.as_table().unwrap().keys())
              .map(|key| key.as_str().to_string())
          };
          let remaining_dependencies = dependency_names("dependencies")
            .chain(dependency_names("build-dependencies"))
            .collect::<HashSet<_>>();
          let unique_dev_dependencies =
            dev_dependencies.difference(&remaining_dependencies).cloned().collect::<HashSet<_>>();
          remove_dependencies_from_features(&mut crate_toml, &unique_dev_dependencies);
        }

        // Remove all benches
        crate_toml.remove("bench");
        // Remove all examples
        crate_toml.remove("example");

        fs::write(crate_toml_path, toml::to_string_pretty(&crate_toml).unwrap().as_bytes())
          .unwrap();
      }
    }

    "upgrade" => {
      let dep = args.next().unwrap();
      let version = args.next().unwrap();

      let (workspace_toml_path, mut workspace_toml) = workspace_toml();
      let dependencies = workspace_toml["workspace"]["dependencies"].as_table_mut().unwrap();
      let dep = &mut dependencies[&dep];
      if let Some(dep) = dep.as_table_mut() {
        dep["version"] = version.to_string().into();
      } else {
        *dep = version.to_string().into();
      }
      fs::write(workspace_toml_path, toml::to_string_pretty(&workspace_toml).unwrap().as_bytes())
        .unwrap();
    }

    _ => panic!("unknown command"),
  }
}
