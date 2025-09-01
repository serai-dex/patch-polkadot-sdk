use std::{path::Path, fs, collections::HashSet};

/// Remove dependencies from use within the crate's features.
pub(crate) fn remove_dependencies_from_features(
  crate_toml: &mut toml::Table,
  dependencies: &HashSet<String>,
) {
  if let Some(features) = crate_toml.get_mut("features") {
    let features = features.as_table_mut().unwrap();
    for (_key, value) in features {
      let value = value.as_array_mut().unwrap();
      crate::remove_from_array(value, |feature| {
        let without_feature_spec = feature.split('/').next().unwrap();
        let without_optional_flag = without_feature_spec.split('?').next().unwrap();
        dependencies.contains(without_optional_flag)
      });
    }
  }
}

pub(crate) fn remove_dependencies(
  crate_toml_path: impl AsRef<Path>,
  mut dependencies: HashSet<String>,
) {
  let crate_toml = fs::read_to_string(crate_toml_path.as_ref()).unwrap();
  let mut crate_toml = toml::from_str::<toml::Table>(&crate_toml).unwrap();

  for table in ["dependencies", "build-dependencies", "dev-dependencies"] {
    let Some(table) = crate_toml.get_mut(table) else { continue };

    let toml::Value::Table(table) = table else {
      panic!("table wasn't a table");
    };

    let remaining = table.keys().map(|key| key.as_str().to_string()).collect::<Vec<_>>();
    for remaining in remaining {
      // If this dependency is to be removed, remove it
      if dependencies.contains(&remaining) {
        table.remove(&remaining);
        continue;
      }

      // If this dependency is an alias for a removed crate, remove it
      if let Some(table) = table[&remaining].as_table_mut() {
        if let Some(package) = table.get("package") {
          let package = package.as_str().unwrap();
          if dependencies.contains(package) {
            table.remove(&remaining);
            // Mark this alias a removed dependency
            dependencies.insert(remaining);
          }
        }
      }
    }
  }
  remove_dependencies_from_features(&mut crate_toml, &dependencies);

  fs::write(crate_toml_path, toml::to_string_pretty(&crate_toml).unwrap().as_bytes()).unwrap();
}
