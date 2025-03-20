use std::sync::Mutex;

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct Settings {
    pub access_token: Option<String>,
}

const PATH: &str = "/config/baam/settings.json";

pub static SETTINGS: SettingsHolder = SettingsHolder::new();

pub struct SettingsHolder {
    settings: Mutex<Option<Settings>>,
}

impl SettingsHolder {
    pub const fn new() -> Self {
        SettingsHolder {
            settings: Mutex::new(None),
        }
    }

    pub fn get(&self) -> Settings {
        let mut guard = self.settings.lock().unwrap();

        if let Some(settings) = guard.as_ref() {
            settings.clone()
        } else {
            let settings = Self::load();
            *guard = Some(settings.clone());
            settings
        }
    }

    pub fn modify(&self, f: impl FnOnce(&mut Settings)) {
        let mut guard = self.settings.lock().unwrap();

        let settings = if let Some(settings) = guard.as_mut() {
            settings
        } else {
            *guard = Some(Self::load());

            guard.as_mut().unwrap()
        };
        f(settings);
        Self::save(settings);
    }

    fn load() -> Settings {
        match std::fs::read(PATH) {
            Ok(settings_json) => {
                serde_json::from_slice(&settings_json).expect("Failed to parse settings json")
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Settings::default(),
            Err(e) => {
                panic!("Failed to read settings file: {}", e)
            }
        }
    }

    fn save(settings: &Settings) {
        let path = std::path::Path::new(PATH);
        if let Some(parent_path) = path.parent() {
            std::fs::create_dir_all(parent_path).expect("Failed to create settings directory");
        }

        let settings_json =
            serde_json::to_vec_pretty(settings).expect("Failed to serialize settings");

        std::fs::write(path, settings_json).expect("Failed to write settings file");
    }
}
