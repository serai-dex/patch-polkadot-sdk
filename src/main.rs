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

    "remove_dependency" => {
      let dep = HashSet::from([args.next().unwrap()]);
      let remaining_crates = discover_all_crates_in_folder(".");
      for item in remaining_crates {
        let crate_toml_path = item.path.join("Cargo.toml");
        remove_dependencies(crate_toml_path, dep.clone());
      }
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

      // Also remove dev-only `opt-level = 3` which slow down compilation
      let (workspace_toml_path, mut workspace_toml) = workspace_toml();
      let table = workspace_toml["profile"]["dev"]["package"].as_table_mut().unwrap();
      for key in table.keys().map(Clone::clone).collect::<HashSet<_>>() {
        table[&key].as_table_mut().unwrap().remove("opt-level");
        if table[&key].as_table().unwrap().is_empty() {
          table.remove(&key);
        }
      }
      fs::write(workspace_toml_path, toml::to_string_pretty(&workspace_toml).unwrap().as_bytes())
        .unwrap();
    }

    "upgrade" => {
      let dep = args.next().unwrap();
      let version = args.next().unwrap();
      let upgrade_to = semver::Version::parse(&version).unwrap();

      let (workspace_toml_path, mut workspace_toml) = workspace_toml();
      let dependencies = workspace_toml["workspace"]["dependencies"].as_table_mut().unwrap();
      let dep = &mut dependencies[&dep];
      if let Some(dep) = dep.as_table_mut() {
        let spec = semver::VersionReq::parse(dep["version"].as_str().unwrap()).unwrap();
        assert_eq!(spec.comparators.len(), 1);
        assert!(matches!(spec.comparators[0].op, semver::Op::Caret));
        if (spec.comparators[0].major > upgrade_to.major) ||
          ((spec.comparators[0].major == upgrade_to.major) &&
            (spec.comparators[0].minor.unwrap_or(0) > upgrade_to.minor))
        {
          panic!("upgrading {} to a lesser version", dep);
        }
        dep["version"] = version.to_string().into();
      } else {
        let spec = semver::VersionReq::parse(dep.as_str().unwrap()).unwrap();
        assert_eq!(spec.comparators.len(), 1);
        assert!(matches!(spec.comparators[0].op, semver::Op::Caret));
        if (spec.comparators[0].major > upgrade_to.major) ||
          ((spec.comparators[0].major == upgrade_to.major) &&
            (spec.comparators[0].minor.unwrap_or(0) > upgrade_to.minor))
        {
          panic!("upgrading {} to a lesser version", dep);
        }
        *dep = version.to_string().into();
      }
      fs::write(workspace_toml_path, toml::to_string_pretty(&workspace_toml).unwrap().as_bytes())
        .unwrap();
    }

    "machete" => {
      // Add an ignore statement for `machete`'s false positives
      let (workspace_toml_path, _) = workspace_toml();
      const IGNORE: &str = "
      [workspace.metadata.cargo-machete]
      ignored = [\"log\", \"parity-scale-codec\", \"codec\", \"sp-debug-derive\"]
      ";
      fs::write(&workspace_toml_path, fs::read_to_string(&workspace_toml_path).unwrap() + IGNORE)
        .unwrap();

      // Have `machete` apply the fix
      std::process::Command::new("cargo").args(["machete", "--fix"]).output().unwrap();

      // Remove the ignore statement to tidy up
      let mut table =
        toml::from_str::<toml::Table>(&fs::read_to_string(&workspace_toml_path).unwrap()).unwrap();
      table["workspace"].as_table_mut().unwrap().remove("metadata");
      fs::write(&workspace_toml_path, toml::to_string_pretty(&table).unwrap()).unwrap();

      // Remove removed dependencies from inclusion in features
      for item in discover_all_crates_in_folder("substrate") {
        let crate_toml_path = item.path.join("Cargo.toml");

        let mut crate_toml =
          toml::from_str::<toml::Table>(&fs::read_to_string(&crate_toml_path).unwrap()).unwrap();
        let deps = crate_toml
          .get("dependencies")
          .map(|deps| deps.as_table().unwrap().keys().cloned())
          .into_iter()
          .chain(
            crate_toml
              .get("features")
              .map(|features| features.as_table().unwrap().keys().cloned())
              .into_iter(),
          )
          .flatten()
          .collect::<HashSet<_>>();
        if let Some(features) = crate_toml.get_mut("features") {
          let features = features.as_table_mut().unwrap();
          for (_feature, enables) in features.iter_mut() {
            let enables = enables.as_array_mut().unwrap();
            remove_from_array(enables, |enabled| {
              !deps.contains(
                enabled
                  .split("/")
                  .next()
                  .unwrap()
                  .split("?")
                  .next()
                  .unwrap()
                  .trim_start_matches("dep:"),
              )
            });
          }
        }

        fs::write(crate_toml_path, toml::to_string_pretty(&crate_toml).unwrap().as_bytes())
          .unwrap();
      }
    }

    "trim_workspace_dependencies" => {
      let all_crates_in_workspace = discover_all_crates_in_folder("substrate")
        .into_iter()
        .map(|dep| dep.name)
        .collect::<HashSet<_>>();

      let all_deps_in_lock_file = {
        let mut root_dir = env::current_dir().unwrap();
        while !root_dir.ends_with("polkadot-sdk") {
          assert!(
            root_dir.pop(),
            "couldn't find polkadot-sdk directory at or above current directory"
          );
        }
        let lock_path = root_dir.join("Cargo.lock");
        let lock_toml = fs::read_to_string(&lock_path).unwrap();

        let mut all = HashSet::with_capacity(1000);
        for package in
          toml::from_str::<toml::Table>(&lock_toml).unwrap()["package"].as_array().unwrap()
        {
          let package = package.as_table().unwrap();
          if !all_crates_in_workspace.contains(package["name"].as_str().unwrap()) {
            continue;
          }
          for dep in package.get("dependencies").and_then(toml::Value::as_array).unwrap_or(&vec![])
          {
            all.insert(dep.as_str().unwrap().split(' ').next().unwrap().to_owned());
          }
        }
        all
      };

      let (workspace_toml_path, mut workspace_toml) = workspace_toml();

      let dependencies = workspace_toml["workspace"]["dependencies"].as_table_mut().unwrap();
      for key in dependencies.keys().map(Clone::clone).collect::<HashSet<_>>() {
        if !all_deps_in_lock_file.contains(&key) {
          // Check if it's an alias for a present package
          if let Some(dep) = dependencies[&key].as_table() {
            if let Some(dep) = dep.get("package") {
              if all_deps_in_lock_file.contains(dep.as_str().unwrap()) {
                continue;
              }
            }
          }

          // Remove it, as it's unused
          dependencies.remove(&key);
        }
      }

      fs::write(workspace_toml_path, toml::to_string_pretty(&workspace_toml).unwrap().as_bytes())
        .unwrap();
    }

    "remove_trait" => {
      let folder = args.next().unwrap();
      let trait_t = args.next().unwrap();
      let mut queue = vec![folder];
      while let Some(folder) = queue.pop() {
        for file in fs::read_dir(folder).unwrap() {
          let file = file.unwrap();
          if file.file_type().unwrap().is_dir() {
            queue.push(file.path().into_os_string().to_str().unwrap().to_owned());
          }
          if file.file_name().to_str().unwrap().ends_with(".rs") {
            let contents = fs::read_to_string(file.path()).unwrap();
            let mut keep: Vec<&str> = vec![];
            let mut lines = contents.lines();
            while let Some(mut line) = lines.next() {
              if let Some(first) = line.split_whitespace().next() {
                // If this is `trait trait_t` or `impl trait_t`...
                let is_impl = first.split('<').next().unwrap() == "impl";
                let is_access = first.split('(').next().unwrap() == "pub";
                let is_trait = {
                  let mut first = first;
                  if is_access {
                    first = line.split_whitespace().nth(1).unwrap();
                  }
                  first == "trait"
                };

                let count_brackets = |line: &str| {
                  let left_brackets = line.chars().filter(|c| *c == '<').count();
                  let right_brackets = line.chars().filter(|c| *c == '>').count();
                  let mut arrows = 0;
                  let mut line = line.chars();
                  let mut last = line.next();
                  for next in line {
                    if (next == '>') && (last.unwrap() == '-') {
                      arrows += 1;
                    }
                    last = Some(next);
                  }
                  left_brackets - (right_brackets - arrows)
                };

                if is_impl || is_trait {
                  // Find the name of the implemented trait
                  let name = {
                    let mut name = None;

                    // Potentially one-line
                    if count_brackets(line) == 0 {
                      let remaining = if is_access {
                        line.split_whitespace().skip(1).collect::<Vec<_>>().join(" ")
                      } else {
                        line.trim_start().to_owned()
                      };
                      let mut brackets = 0;
                      let mut last_char = None;
                      for (i, c) in remaining.char_indices() {
                        if (brackets == 0) && (c == ' ') {
                          name =
                            Some(remaining[i ..].split_whitespace().next().unwrap().to_owned());
                          break;
                        }
                        if c == '<' {
                          brackets += 1;
                        }
                        if (c == '>') && (last_char != Some('-')) {
                          brackets -= 1;
                        }
                        last_char = Some(c);
                      }
                    }

                    // Multi-line
                    if name.is_none() {
                      let mut brackets = count_brackets(line);
                      let remaining = lines.clone().collect::<Vec<_>>().join("\n");
                      let mut last_char = None;
                      for (i, c) in remaining.char_indices() {
                        if brackets == 0 {
                          name =
                            Some(remaining[i ..].split_whitespace().next().unwrap().to_owned());
                          break;
                        }
                        if c == '<' {
                          brackets += 1;
                        }
                        if (c == '>') && (last_char != Some('-')) {
                          brackets -= 1;
                        }
                        last_char = Some(c);
                      }
                    }
                    name.unwrap()
                  };

                  if name.split("::").last().unwrap() == trait_t {
                    // Remove any directly preceding attributes/comments/whitespace
                    while let Some(prior) = keep.last() {
                      let prior = prior.split_whitespace().next().unwrap_or("");
                      if prior.starts_with("//") || prior.starts_with("#[") || prior.is_empty() {
                        keep.pop();
                        continue;
                      }
                      break;
                    }

                    // Find its opening bracket, which may be on another line
                    while !line.contains("{") {
                      line = lines.next().unwrap();
                    }
                    let mut brackets = line.chars().filter(|c| *c == '{').count() -
                      line.chars().filter(|c| *c == '}').count();
                    // Find its closing network
                    // This expects brackets to not be nested on the same line nor commented
                    while brackets != 0 {
                      line = lines.next().unwrap();
                      if line.contains("{") {
                        brackets += 1;
                      }
                      if line.contains("}") {
                        brackets -= 1;
                      }
                    }

                    // Advance past the line with the closing bracket
                    line = lines.next().unwrap();
                  }
                }
              }
              // Push any lines we don't skip for being an `impl trait_t`
              keep.push(line);
            }
            // Write the lines we're keeping
            fs::write(file.path(), keep.join("\n").trim_end().to_owned() + "\n").unwrap();
          }
        }
      }
    }

    _ => panic!("unknown command"),
  }
}
