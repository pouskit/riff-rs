//! Raccourcis `.lnk` et identité de l'application dans la barre des tâches (Windows).

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{
  CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
  IPersistFile,
};
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{IShellLinkW, SetCurrentProcessExplicitAppUserModelID, ShellLink};
use windows::core::{Interface, PCWSTR};

fn wide(text: &OsStr) -> Vec<u16> {
  text.encode_wide().chain(std::iter::once(0)).collect()
}

/// Identité du processus dans la barre des tâches : les fenêtres se regroupent avec le raccourci de même identité.
/// À appeler avant la création de la première fenêtre.
pub fn set_process_app_id(app_id: &str) {
  let id = wide(OsStr::new(app_id));
  // SAFETY: `id` est terminé par un nul et vit pendant l'appel, qui copie la chaîne.
  if let Err(error) = unsafe { SetCurrentProcessExplicitAppUserModelID(PCWSTR(id.as_ptr())) } {
    tracing::warn!(%error, "identité de la barre des tâches non posée");
  }
}

/// Écrit un `.lnk` vers `target` (dossier de travail et icône : l'exécutable), avec son identité de barre des tâches.
/// COM exige son propre fil à appartement monofil.
pub fn create_shortcut(
  lnk: &Path,
  target: &Path,
  description: &str,
  app_id: &str,
) -> Result<(), String> {
  let (lnk, target, description, app_id) = (
    lnk.to_path_buf(),
    target.to_path_buf(),
    description.to_string(),
    app_id.to_string(),
  );
  std::thread::spawn(move || {
    // SAFETY: COM est initialisé et libéré sur ce fil, les objets COM ne le quittent pas.
    unsafe {
      let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
      let result =
        write_link(&lnk, &target, &description, &app_id).map_err(|error| error.to_string());
      if initialized {
        CoUninitialize();
      }
      result
    }
  })
  .join()
  .map_err(|_| "shortcut thread panicked".to_string())?
}

unsafe fn write_link(
  lnk: &Path,
  target: &Path,
  description: &str,
  app_id: &str,
) -> windows::core::Result<()> {
  // SAFETY: toutes les chaînes passées sont terminées par un nul et vivent jusqu'à la fin de la fonction.
  unsafe {
    let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
    let target_w = wide(target.as_os_str());
    let dir_w = wide(target.parent().unwrap_or(Path::new("")).as_os_str());
    let description_w = wide(OsStr::new(description));
    link.SetPath(PCWSTR(target_w.as_ptr()))?;
    link.SetWorkingDirectory(PCWSTR(dir_w.as_ptr()))?;
    link.SetDescription(PCWSTR(description_w.as_ptr()))?;
    link.SetIconLocation(PCWSTR(target_w.as_ptr()), 0)?;

    let store: IPropertyStore = link.cast()?;
    // La chaîne est allouée par COM et libérée par `Drop` de `PROPVARIANT` (`PropVariantClear`).
    let value = PROPVARIANT::from(app_id);
    store.SetValue(&PKEY_AppUserModel_ID, &value)?;
    store.Commit()?;

    let file: IPersistFile = link.cast()?;
    file.Save(PCWSTR(wide(lnk.as_os_str()).as_ptr()), true)
  }
}

/// Cible et identité lues dans un `.lnk`, pour les essais.
#[cfg(test)]
pub fn read_shortcut(lnk: &Path) -> Result<(String, String), String> {
  let lnk = lnk.to_path_buf();
  std::thread::spawn(move || {
    // SAFETY: même contrat que `create_shortcut`.
    unsafe {
      let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
      let result = read_link(&lnk).map_err(|error| error.to_string());
      if initialized {
        CoUninitialize();
      }
      result
    }
  })
  .join()
  .map_err(|_| "thread panicked".to_string())?
}

#[cfg(test)]
unsafe fn read_link(lnk: &Path) -> windows::core::Result<(String, String)> {
  use windows::Win32::Storage::FileSystem::WIN32_FIND_DATAW;
  use windows::Win32::System::Com::STGM_READ;
  // SAFETY: tampons locaux de taille connue, chaînes terminées par un nul.
  unsafe {
    let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
    let file: IPersistFile = link.cast()?;
    file.Load(PCWSTR(wide(lnk.as_os_str()).as_ptr()), STGM_READ)?;
    let mut path = [0u16; 520];
    link.GetPath(&mut path, std::ptr::null_mut::<WIN32_FIND_DATAW>(), 0)?;
    let store: IPropertyStore = link.cast()?;
    let value = store.GetValue(&PKEY_AppUserModel_ID)?;
    let pointer = value.Anonymous.Anonymous.Anonymous.pwszVal;
    let app_id = if pointer.is_null() {
      String::new()
    } else {
      pointer.to_string().unwrap_or_default()
    };
    let end = path
      .iter()
      .position(|unit| *unit == 0)
      .unwrap_or(path.len());
    Ok((String::from_utf16_lossy(&path[..end]), app_id))
  }
}
