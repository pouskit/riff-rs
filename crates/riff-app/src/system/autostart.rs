//! Démarrage avec la session Windows : valeur `Run` de `HKCU`, lancement dans la zone de notification (`--tray`).
//! La présence de la valeur, non désactivée dans `StartupApproved` (Gestionnaire des tâches, Paramètres), est la seule
//! source de vérité de la case du menu.

use std::path::Path;

/// Argument d'un lancement automatique : la fenêtre principale reste cachée.
pub const TRAY_ARG: &str = "--tray";
/// Nom stable de la valeur du registre.
pub const VALUE_NAME: &str = "Riff";
const DATA_DIR_ENV: &str = "RIFF_DATA_DIR";

/// État de l'entrée « Start with Windows ».
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum State {
  /// Grisée : autre système que Windows, ou profil d'essai.
  #[default]
  Unavailable,
  Off,
  On,
}

/// Valeur de démarrage automatique. Le registre réel passe par [`system_key`], les essais par un double.
pub trait RunKey {
  fn read(&self) -> Option<String>;
  fn write(&self, command: &str) -> Result<(), String>;
  fn remove(&self) -> Result<(), String>;
  /// Faux si l'utilisateur a désactivé l'entrée dans Windows (`StartupApproved`) : la valeur existe mais ne démarre plus.
  fn approved(&self) -> bool;
  /// Lève la désactivation de Windows en supprimant l'entrée `StartupApproved`.
  fn approve(&self) -> Result<(), String>;
}

/// Ligne de commande de la valeur : chemin de l'exécutable entre guillemets, puis l'argument de lancement.
#[must_use]
pub fn command_for(exe: &Path) -> String {
  format!("\"{}\" {TRAY_ARG}", exe.display())
}

/// Lancement de session : l'argument `--tray` est présent.
#[must_use]
pub fn is_tray_launch(args: &[String]) -> bool {
  args.iter().any(|arg| arg == TRAY_ARG)
}

/// Exécutable visé par une valeur `Run` : le chemin entre guillemets, sinon le premier mot.
fn exe_of(command: &str) -> &str {
  let command = command.trim();
  match command.strip_prefix('"') {
    Some(rest) => rest.split('"').next().unwrap_or(rest),
    None => command.split(' ').next().unwrap_or(command),
  }
}

/// Premier octet de la valeur binaire `StartupApproved` : bit 0 levé (03, 07) pour une entrée désactivée.
fn disabled_by_windows(data: &[u8]) -> bool {
  data.first().is_some_and(|byte| byte & 1 == 1)
}

/// Les profils d'essai (`RIFF_DATA_DIR`, `RIFF_IPC_NAME`) ne s'inscrivent jamais au démarrage.
fn profile_isolated(is_set: impl Fn(&str) -> bool) -> bool {
  is_set(DATA_DIR_ENV) || is_set(super::channel::NAME_OVERRIDE_ENV)
}

fn state_of(key: &dyn RunKey) -> State {
  if key.read().is_some() && key.approved() {
    State::On
  } else {
    State::Off
  }
}

/// Inscrit ou retire la valeur, puis relit le registre : l'état rendu est celui qui existe vraiment.
fn set(key: &dyn RunKey, exe: &Path, on: bool) -> State {
  let result = if on {
    key.write(&command_for(exe)).and_then(|()| key.approve())
  } else {
    key.remove()
  };
  if let Err(error) = result {
    tracing::warn!(%error, on, "démarrage avec Windows non modifié");
  }
  state_of(key)
}

fn toggled(key: &dyn RunKey, exe: &Path) -> State {
  set(key, exe, state_of(key) == State::Off)
}

/// Inscrit le chemin actuel si la valeur existe mais vise un exécutable disparu (programme déplacé).
/// Un exécutable encore présent n'est jamais remplacé : un build de dev ou une copie portable ne volent pas la place.
fn refresh(key: &dyn RunKey, exe: &Path, exists: impl Fn(&Path) -> bool) {
  let expected = command_for(exe);
  if key
    .read()
    .is_some_and(|stored| stored != expected && !exists(Path::new(exe_of(&stored))))
    && let Err(error) = key.write(&expected)
  {
    tracing::warn!(%error, "chemin du démarrage avec Windows non mis à jour");
  }
}

/// Clé réelle, sauf profil d'essai et hors Windows.
fn system_key() -> Option<Box<dyn RunKey>> {
  if profile_isolated(|name| std::env::var_os(name).is_some()) {
    return None;
  }
  #[cfg(windows)]
  {
    Some(Box::new(registry::Run))
  }
  #[cfg(not(windows))]
  {
    None
  }
}

#[must_use]
pub fn current_state() -> State {
  system_key().map_or(State::Unavailable, |key| state_of(&*key))
}

/// Bascule l'entrée et rend l'état relu.
pub fn toggle() -> State {
  match (system_key(), std::env::current_exe()) {
    (Some(key), Ok(exe)) => toggled(&*key, &exe),
    (Some(key), Err(_)) => state_of(&*key),
    (None, _) => State::Unavailable,
  }
}

/// Au lancement : réécrit la valeur si l'exécutable a changé d'emplacement.
pub fn refresh_at_launch() {
  if let (Some(key), Ok(exe)) = (system_key(), std::env::current_exe()) {
    refresh(&*key, &exe, Path::exists);
  }
}

/// La fenêtre principale reste cachée : lancement `--tray` avec une zone de notification qui va exister.
#[must_use]
pub const fn starts_hidden(tray_arg: bool, tray_expected: bool) -> bool {
  tray_arg && tray_expected
}

#[cfg(windows)]
mod registry {
  use super::{RunKey, VALUE_NAME};
  use std::ffi::OsStr;
  use std::os::windows::ffi::OsStrExt;
  use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
  use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RRF_RT_REG_BINARY, RRF_RT_REG_SZ, RegCloseKey,
    RegDeleteValueW, RegGetValueW, RegOpenKeyExW, RegSetValueExW,
  };
  use windows::core::PCWSTR;

  const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
  const APPROVED_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

  fn wide(text: &str) -> Vec<u16> {
    OsStr::new(text)
      .encode_wide()
      .chain(std::iter::once(0))
      .collect()
  }

  pub struct Run;

  /// Ouvre la clé en écriture et la referme après `action`, qui rend un code d'erreur Win32.
  /// Une clé absente n'est pas une erreur quand il ne reste rien à y retirer (`missing_ok`).
  fn with_key(
    key_path: &str,
    missing_ok: bool,
    action: impl FnOnce(HKEY) -> u32,
  ) -> Result<(), String> {
    let path = wide(key_path);
    let mut key = HKEY::default();
    // SAFETY: `path` est terminé par un nul et vit pendant l'appel ; `key` est une sortie valide.
    let opened = unsafe {
      RegOpenKeyExW(
        HKEY_CURRENT_USER,
        PCWSTR(path.as_ptr()),
        None,
        KEY_SET_VALUE,
        &mut key,
      )
    };
    if missing_ok && opened == ERROR_FILE_NOT_FOUND {
      return Ok(());
    }
    if opened.0 != 0 {
      return Err(format!("RegOpenKeyExW: {}", opened.0));
    }
    let code = action(key);
    // SAFETY: `key` vient d'être ouverte avec succès.
    unsafe {
      let _ = RegCloseKey(key);
    }
    if code == 0 || code == ERROR_FILE_NOT_FOUND.0 {
      Ok(())
    } else {
      Err(format!("code {code}"))
    }
  }

  impl RunKey for Run {
    fn read(&self) -> Option<String> {
      let path = wide(RUN_KEY);
      let name = wide(VALUE_NAME);
      let mut size = 0u32;
      // SAFETY: chaînes terminées par un nul ; sans tampon, l'appel ne fait que rendre la taille en octets.
      let probe = unsafe {
        RegGetValueW(
          HKEY_CURRENT_USER,
          PCWSTR(path.as_ptr()),
          PCWSTR(name.as_ptr()),
          RRF_RT_REG_SZ,
          None,
          None,
          Some(&mut size),
        )
      };
      if probe.0 != 0 || size == 0 {
        return None;
      }
      let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
      // SAFETY: le tampon couvre au moins `size` octets.
      let read = unsafe {
        RegGetValueW(
          HKEY_CURRENT_USER,
          PCWSTR(path.as_ptr()),
          PCWSTR(name.as_ptr()),
          RRF_RT_REG_SZ,
          None,
          Some(buffer.as_mut_ptr().cast()),
          Some(&mut size),
        )
      };
      if read.0 != 0 {
        return None;
      }
      let end = buffer
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(buffer.len());
      Some(String::from_utf16_lossy(&buffer[..end]))
    }

    fn write(&self, command: &str) -> Result<(), String> {
      let name = wide(VALUE_NAME);
      let data: Vec<u8> = wide(command)
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect();
      with_key(RUN_KEY, false, |key| {
        // SAFETY: `name` est terminé par un nul ; `data` contient la chaîne et son nul final.
        unsafe { RegSetValueExW(key, PCWSTR(name.as_ptr()), None, REG_SZ, Some(&data)).0 }
      })
    }

    fn remove(&self) -> Result<(), String> {
      let name = wide(VALUE_NAME);
      with_key(RUN_KEY, false, |key| {
        // SAFETY: `name` est terminé par un nul et vit pendant l'appel.
        unsafe { RegDeleteValueW(key, PCWSTR(name.as_ptr())).0 }
      })
    }

    fn approved(&self) -> bool {
      let path = wide(APPROVED_KEY);
      let name = wide(VALUE_NAME);
      let mut data = [0u8; 16];
      let mut size = data.len() as u32;
      // SAFETY: chaînes terminées par un nul ; `data` couvre `size` octets. Une valeur plus longue échoue : approuvée.
      let read = unsafe {
        RegGetValueW(
          HKEY_CURRENT_USER,
          PCWSTR(path.as_ptr()),
          PCWSTR(name.as_ptr()),
          RRF_RT_REG_BINARY,
          None,
          Some(data.as_mut_ptr().cast()),
          Some(&mut size),
        )
      };
      read.0 != 0 || !super::disabled_by_windows(&data[..size as usize])
    }

    fn approve(&self) -> Result<(), String> {
      let name = wide(VALUE_NAME);
      with_key(APPROVED_KEY, true, |key| {
        // SAFETY: `name` est terminé par un nul et vit pendant l'appel.
        unsafe { RegDeleteValueW(key, PCWSTR(name.as_ptr())).0 }
      })
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::cell::RefCell;
  use std::path::PathBuf;

  #[derive(Default)]
  struct Fake(RefCell<Option<String>>, RefCell<bool>);

  impl RunKey for Fake {
    fn read(&self) -> Option<String> {
      self.0.borrow().clone()
    }
    fn write(&self, command: &str) -> Result<(), String> {
      *self.0.borrow_mut() = Some(command.to_string());
      Ok(())
    }
    fn remove(&self) -> Result<(), String> {
      *self.0.borrow_mut() = None;
      Ok(())
    }
    fn approved(&self) -> bool {
      !*self.1.borrow()
    }
    fn approve(&self) -> Result<(), String> {
      *self.1.borrow_mut() = false;
      Ok(())
    }
  }

  /// Écriture refusée : la relecture dit la vérité.
  struct Denied;

  impl RunKey for Denied {
    fn read(&self) -> Option<String> {
      None
    }
    fn write(&self, _: &str) -> Result<(), String> {
      Err("refusé".into())
    }
    fn remove(&self) -> Result<(), String> {
      Err("refusé".into())
    }
    fn approved(&self) -> bool {
      true
    }
    fn approve(&self) -> Result<(), String> {
      Err("refusé".into())
    }
  }

  fn exe() -> PathBuf {
    PathBuf::from(r"C:\Program Files\Riff\riff-app.exe")
  }

  #[test]
  fn the_command_quotes_the_path_and_adds_the_tray_argument() {
    assert_eq!(
      command_for(&exe()),
      r#""C:\Program Files\Riff\riff-app.exe" --tray"#
    );
  }

  #[test]
  fn the_checkbox_follows_the_presence_of_the_value() {
    let key = Fake::default();
    assert_eq!(state_of(&key), State::Off);
    assert_eq!(toggled(&key, &exe()), State::On);
    assert_eq!(key.read(), Some(command_for(&exe())));
    assert_eq!(toggled(&key, &exe()), State::Off);
    assert_eq!(key.read(), None);
  }

  #[test]
  fn a_value_written_by_someone_else_still_counts_as_on() {
    let key = Fake(
      RefCell::new(Some("autre commande".into())),
      RefCell::default(),
    );
    assert_eq!(state_of(&key), State::On);
    assert_eq!(toggled(&key, &exe()), State::Off);
  }

  #[test]
  fn a_refused_write_leaves_the_state_off() {
    assert_eq!(toggled(&Denied, &exe()), State::Off);
  }

  #[test]
  fn a_missing_executable_is_replaced_only_when_the_value_exists() {
    let key = Fake::default();
    refresh(&key, &exe(), |_| false);
    assert_eq!(key.read(), None);
    key.write(r#""D:\ancien\riff-app.exe" --tray"#).unwrap();
    refresh(&key, &exe(), |_| false);
    assert_eq!(key.read(), Some(command_for(&exe())));
  }

  #[test]
  fn an_executable_that_still_exists_keeps_its_place() {
    let key = Fake::default();
    let stored = r#""D:\ancien\riff-app.exe" --tray"#;
    key.write(stored).unwrap();
    refresh(&key, &exe(), |path| {
      path == Path::new(r"D:\ancien\riff-app.exe")
    });
    assert_eq!(key.read().as_deref(), Some(stored));
  }

  #[test]
  fn the_executable_is_read_from_the_stored_command() {
    assert_eq!(exe_of(r#""C:\a b\riff.exe" --tray"#), r"C:\a b\riff.exe");
    assert_eq!(exe_of(r"C:\riff.exe --tray"), r"C:\riff.exe");
  }

  #[test]
  fn an_entry_disabled_in_windows_shows_unchecked_and_enabling_clears_it() {
    let key = Fake(RefCell::new(Some(command_for(&exe()))), RefCell::new(true));
    assert_eq!(state_of(&key), State::Off);
    assert_eq!(toggled(&key, &exe()), State::On);
    assert!(key.approved());
    assert!(disabled_by_windows(&[0x03, 0, 0, 0]));
    assert!(!disabled_by_windows(&[0x02, 0, 0, 0]));
    assert!(!disabled_by_windows(&[]));
  }

  #[test]
  fn only_the_tray_argument_marks_a_session_launch() {
    let args = |list: &[&str]| {
      list
        .iter()
        .map(|arg| (*arg).to_string())
        .collect::<Vec<_>>()
    };
    assert!(is_tray_launch(&args(&["--tray"])));
    assert!(!is_tray_launch(&args(&["a.mp3", "--dev"])));
  }

  #[test]
  fn a_test_profile_is_isolated_from_the_registry() {
    assert!(!profile_isolated(|_| false));
    assert!(profile_isolated(|name| name == "RIFF_DATA_DIR"));
    assert!(profile_isolated(|name| name == "RIFF_IPC_NAME"));
  }

  #[test]
  fn the_window_stays_hidden_only_for_a_tray_launch_with_a_tray() {
    assert!(starts_hidden(true, true));
    assert!(!starts_hidden(true, false));
    assert!(!starts_hidden(false, true));
    assert!(!starts_hidden(false, false));
  }
}
