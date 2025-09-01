use std::{path::PathBuf, fs, env};

// TODO: RAII to auto-save, ensure only one handle is held at a time
pub(crate) fn workspace_toml() -> (PathBuf, toml::Table) {
  let mut root_dir = env::current_dir().unwrap();
  while !root_dir.ends_with("polkadot-sdk") {
    assert!(root_dir.pop(), "couldn't find polkadot-sdk directory at or above current directory");
  }
  let workspace_toml_path = root_dir.join("Cargo.toml");
  let workspace_toml = fs::read_to_string(&workspace_toml_path).unwrap();
  (workspace_toml_path, toml::from_str::<toml::Table>(&workspace_toml).unwrap())
}

pub(crate) fn remove_from_default_members(toml: &mut toml::Table, path: &str) {
  let workspace = toml["workspace"].as_table_mut().unwrap();
  let is_empty = {
    let Some(members) = workspace.get_mut("default-members") else { return };
    let members = members.as_array_mut().unwrap();
    crate::remove_from_array(members, |member| member.starts_with(path));
    members.is_empty()
  };
  if is_empty {
    workspace.remove("default-members");
  }
}
