//! Reading OpenDeck's profile files, to tell whether the deck's selected
//! profile has anything on it.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use serde::Deserialize;

/// The plugin's own folder (the one holding manifest.json).
pub static PLUGIN_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
	let exe = std::env::current_exe().unwrap_or_default();
	exe.ancestors()
		.find(|dir| dir.join("manifest.json").is_file())
		.map(Path::to_path_buf)
		.unwrap_or_else(|| exe.parent().unwrap_or(Path::new(".")).to_path_buf())
});

/// OpenDeck's config folder: <config>/plugins/<this plugin>.
static CONFIG_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
	PLUGIN_DIR
		.parent()
		.and_then(Path::parent)
		.map(Path::to_path_buf)
		.unwrap_or_default()
});

#[derive(Deserialize)]
struct DeviceStore {
	selected_profile: Option<String>,
}

#[derive(Deserialize)]
struct Profile {
	#[serde(default)]
	keys: Vec<serde_json::Value>,
	#[serde(default)]
	sliders: Vec<serde_json::Value>,
	#[serde(default)]
	infobars: Vec<serde_json::Value>,
}

/// Whether the profile OpenDeck has selected for this device has no actions
/// on it, or doesn't exist yet.
pub fn selected_profile_is_empty(device_id: &str) -> bool {
	is_empty_in(&CONFIG_DIR, device_id)
}

fn is_empty_in(config_dir: &Path, device_id: &str) -> bool {
	let profiles = config_dir.join("profiles");
	let selected = fs::read_to_string(profiles.join(format!("{device_id}.json")))
		.ok()
		.and_then(|text| serde_json::from_str::<DeviceStore>(&text).ok())
		.and_then(|store| store.selected_profile)
		.unwrap_or_else(|| "Default".to_owned());
	let Some(profile) =
		fs::read_to_string(profiles.join(device_id).join(format!("{selected}.json")))
			.ok()
			.and_then(|text| serde_json::from_str::<Profile>(&text).ok())
	else {
		return true;
	};
	profile
		.keys
		.iter()
		.chain(&profile.sliders)
		.chain(&profile.infobars)
		.all(serde_json::Value::is_null)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn config(selected: &str, profile: &str) -> PathBuf {
		let dir = std::env::temp_dir().join(format!(
			"opendeck-galleon-test-{}-{selected}",
			std::process::id()
		));
		fs::create_dir_all(dir.join("profiles/cg-TEST")).unwrap();
		fs::write(
			dir.join("profiles/cg-TEST.json"),
			format!(r#"{{"selected_profile":"{selected}"}}"#),
		)
		.unwrap();
		fs::write(
			dir.join(format!("profiles/cg-TEST/{selected}.json")),
			profile,
		)
		.unwrap();
		dir
	}

	#[test]
	fn empty_profile_is_empty() {
		let dir = config(
			"Hardware",
			r#"{"keys":[null,null],"sliders":[null,null],"infobars":[]}"#,
		);
		assert!(is_empty_in(&dir, "cg-TEST"));
	}

	#[test]
	fn profile_with_an_action_is_not_empty() {
		let dir = config(
			"Discord",
			r#"{"keys":[null,{"action":{}}],"sliders":[null,null]}"#,
		);
		assert!(!is_empty_in(&dir, "cg-TEST"));
	}

	#[test]
	fn missing_profile_counts_as_empty() {
		assert!(is_empty_in(Path::new("/nonexistent"), "cg-TEST"));
	}
}
