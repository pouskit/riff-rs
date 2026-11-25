//! Appels Windows du canal local : identifiant de l'utilisateur et descripteur de sécurité du tube.

use std::io;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// SID de l'utilisateur du processus, au format `S-1-5-21-...`.
pub fn user_id() -> io::Result<String> {
  let mut token = HANDLE::default();
  // SAFETY: `token` est une sortie valide, le handle est fermé plus bas dans tous les cas.
  unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }
    .map_err(io::Error::other)?;
  let sid = sid_of_token(token);
  // SAFETY: `token` vient d'être ouvert ci-dessus.
  let _ = unsafe { CloseHandle(token) };
  sid
}

fn sid_of_token(token: HANDLE) -> io::Result<String> {
  let mut needed = 0u32;
  // SAFETY: premier appel sans tampon, il ne fait que renseigner la taille demandée.
  let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &mut needed) };
  if needed == 0 {
    return Err(io::Error::other("TokenUser size unknown"));
  }
  // Tampon aligné sur 8 octets : `TOKEN_USER` contient des pointeurs.
  let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
  // SAFETY: le tampon fait au moins `needed` octets.
  unsafe {
    GetTokenInformation(
      token,
      TokenUser,
      Some(buffer.as_mut_ptr().cast()),
      needed,
      &mut needed,
    )
  }
  .map_err(io::Error::other)?;
  // SAFETY: le tampon vient d'être rempli avec un `TOKEN_USER`, aligné par construction.
  let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
  let mut text = windows::core::PWSTR::null();
  // SAFETY: le SID pointe dans `buffer`, vivant jusqu'à la fin de la fonction.
  unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) }.map_err(io::Error::other)?;
  // SAFETY: `text` est une chaîne terminée par un nul allouée par le système.
  let sid = unsafe { text.to_string() }.map_err(io::Error::other);
  // SAFETY: la chaîne a été allouée par `ConvertSidToStringSidW`, qui exige `LocalFree`.
  let _ = unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
  sid
}

/// SDDL d'un descripteur sans héritage qui donne tous les droits au seul utilisateur `sid`.
#[must_use]
pub fn owner_only_sddl(sid: &str) -> String {
  format!("D:P(A;;GA;;;{sid})")
}

/// Contenu de la DACL d'un tube, lu en binaire : indépendant des alias SDDL (`LA`, `BA`, ...).
/// `SystemParametersInfo(SPI_GETCLIENTAREAANIMATION)` : vrai si le système laisse les animations actives. L'appel qui échoue donne
/// « actives », comme `Animation::UpdatePrefersReducedMotion` de Chromium (`ui/gfx/animation/animation_win.cc`).
pub fn client_area_animations() -> bool {
  use windows::Win32::UI::WindowsAndMessaging::{
    SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
  };
  let mut enabled = windows::core::BOOL(1);
  // SAFETY: `enabled` est un BOOL valide pour toute la durée de l'appel, `uiparam` vaut 0 pour cette action.
  let read = unsafe {
    SystemParametersInfoW(
      SPI_GETCLIENTAREAANIMATION,
      0,
      Some((&raw mut enabled).cast()),
      SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
    )
  };
  read.is_err() || enabled.as_bool()
}

#[cfg(test)]
#[derive(Debug)]
pub struct PipeDacl {
  pub protected: bool,
  pub ace_count: u32,
  /// Type de la première ACE (0 = accès autorisé), 0 s'il n'y a aucune ACE.
  pub ace_type: u8,
  pub ace_flags: u8,
  pub mask: u32,
  /// Le SID de la première ACE égale celui du jeton du processus (`EqualSid`).
  pub sid_is_current_user: bool,
}

#[cfg(test)]
fn sid_is_current_user(sid: windows::Win32::Security::PSID) -> io::Result<bool> {
  use windows::Win32::Security::EqualSid;

  let mut token = HANDLE::default();
  // SAFETY: `token` est une sortie valide, le handle est fermé plus bas dans tous les cas.
  unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }
    .map_err(io::Error::other)?;
  let equal = (|| {
    let mut needed = 0u32;
    // SAFETY: premier appel sans tampon, il ne fait que renseigner la taille demandée.
    let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &mut needed) };
    if needed == 0 {
      return Err(io::Error::other("TokenUser size unknown"));
    }
    // Tampon aligné sur 8 octets : `TOKEN_USER` contient des pointeurs.
    let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
    // SAFETY: le tampon fait au moins `needed` octets.
    unsafe {
      GetTokenInformation(
        token,
        TokenUser,
        Some(buffer.as_mut_ptr().cast()),
        needed,
        &mut needed,
      )
    }
    .map_err(io::Error::other)?;
    // SAFETY: le tampon vient d'être rempli avec un `TOKEN_USER`, aligné par construction.
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    // SAFETY: les deux SID sont valides : l'un dans `buffer`, l'autre fourni par l'appelant.
    // `EqualSid` rend une erreur quand les SID diffèrent.
    Ok(unsafe { EqualSid(user.User.Sid, sid) }.is_ok())
  })();
  // SAFETY: `token` vient d'être ouvert ci-dessus.
  let _ = unsafe { CloseHandle(token) };
  equal
}

/// SAFETY: `descriptor` doit être un descripteur valide, vivant pendant l'appel.
#[cfg(test)]
unsafe fn read_dacl(
  descriptor: windows::Win32::Security::PSECURITY_DESCRIPTOR,
) -> io::Result<PipeDacl> {
  use windows::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACL, ACL_SIZE_INFORMATION, AclSizeInformation, GetAce, GetAclInformation,
    GetSecurityDescriptorControl, GetSecurityDescriptorDacl, PSID, SE_DACL_PROTECTED,
  };

  let mut control = 0u16;
  let mut revision = 0u32;
  // SAFETY: `descriptor` est valide (contrat), les sorties sont des variables locales.
  unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) }
    .map_err(io::Error::other)?;
  let protected = control & SE_DACL_PROTECTED.0 != 0;

  let mut present = windows::core::BOOL::default();
  let mut defaulted = windows::core::BOOL::default();
  let mut acl: *mut ACL = std::ptr::null_mut();
  // SAFETY: `descriptor` est valide (contrat), les sorties sont des variables locales.
  unsafe { GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted) }
    .map_err(io::Error::other)?;
  if !present.as_bool() || acl.is_null() {
    return Err(io::Error::other("no DACL"));
  }

  let mut info = ACL_SIZE_INFORMATION::default();
  // SAFETY: `acl` pointe dans `descriptor`, `info` a la taille annoncée.
  unsafe {
    GetAclInformation(
      acl,
      std::ptr::from_mut(&mut info).cast(),
      size_of::<ACL_SIZE_INFORMATION>() as u32,
      AclSizeInformation,
    )
  }
  .map_err(io::Error::other)?;

  let mut dacl = PipeDacl {
    protected,
    ace_count: info.AceCount,
    ace_type: 0,
    ace_flags: 0,
    mask: 0,
    sid_is_current_user: false,
  };
  if info.AceCount == 0 {
    return Ok(dacl);
  }
  let mut ace: *mut core::ffi::c_void = std::ptr::null_mut();
  // SAFETY: l'indice 0 existe (`AceCount` > 0), `acl` est valide.
  unsafe { GetAce(acl, 0, &mut ace) }.map_err(io::Error::other)?;
  let ace = ace.cast::<ACCESS_ALLOWED_ACE>();
  // SAFETY: `ace` pointe sur une ACE de `acl` ; l'en-tête est commun à tous les types d'ACE.
  unsafe {
    dacl.ace_type = (*ace).Header.AceType;
    dacl.ace_flags = (*ace).Header.AceFlags;
  }
  // Type 0 = `ACCESS_ALLOWED_ACE_TYPE` : seul type dont la disposition est `ACCESS_ALLOWED_ACE`.
  if dacl.ace_type == 0 {
    // SAFETY: l'ACE est de type accès autorisé, donc `Mask` puis le SID (à `SidStart`) y figurent.
    unsafe {
      dacl.mask = (*ace).Mask;
      let sid = PSID(std::ptr::addr_of_mut!((*ace).SidStart).cast());
      dacl.sid_is_current_user = sid_is_current_user(sid)?;
    }
  }
  Ok(dacl)
}

/// Lit la DACL du tube nommé `name` (créé par ce processus).
#[cfg(test)]
pub fn dacl_of_pipe(name: &str) -> io::Result<PipeDacl> {
  use std::os::windows::ffi::OsStrExt;
  use windows::Win32::Foundation::{ERROR_SUCCESS, GENERIC_READ};
  use windows::Win32::Security::Authorization::{GetSecurityInfo, SE_KERNEL_OBJECT};
  use windows::Win32::Security::{DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR};
  use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
  };

  const READ_CONTROL: u32 = 0x0002_0000;
  let path: Vec<u16> = std::ffi::OsStr::new(&format!(r"\\.\pipe\{name}"))
    .encode_wide()
    .chain(Some(0))
    .collect();
  // SAFETY: `path` est terminé par un nul, les autres arguments sont des constantes.
  let handle = unsafe {
    CreateFileW(
      windows::core::PCWSTR(path.as_ptr()),
      READ_CONTROL | GENERIC_READ.0,
      FILE_SHARE_READ | FILE_SHARE_WRITE,
      None,
      OPEN_EXISTING,
      FILE_ATTRIBUTE_NORMAL,
      None,
    )
  }
  .map_err(io::Error::other)?;
  let mut descriptor = PSECURITY_DESCRIPTOR::default();
  // SAFETY: `handle` est ouvert, `descriptor` reçoit une allocation libérée plus bas.
  let status = unsafe {
    GetSecurityInfo(
      handle,
      SE_KERNEL_OBJECT,
      DACL_SECURITY_INFORMATION,
      None,
      None,
      None,
      None,
      Some(&mut descriptor),
    )
  };
  let result = if status == ERROR_SUCCESS {
    // SAFETY: `descriptor` vient d'être alloué par `GetSecurityInfo`, libéré après la lecture.
    unsafe { read_dacl(descriptor) }
  } else {
    Err(io::Error::from_raw_os_error(status.0 as i32))
  };
  // SAFETY: `descriptor` a été alloué par `GetSecurityInfo`, `handle` ouvert plus haut.
  unsafe {
    let _ = LocalFree(Some(HLOCAL(descriptor.0)));
    let _ = CloseHandle(handle);
  }
  result
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn user_id_is_a_sid() {
    let sid = user_id().expect("sid");
    assert!(sid.starts_with("S-1-"), "{sid}");
    assert!(
      sid.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
      "{sid}"
    );
  }

  #[test]
  fn sddl_names_one_trustee_and_blocks_inheritance() {
    assert_eq!(owner_only_sddl("S-1-5-21-1"), "D:P(A;;GA;;;S-1-5-21-1)");
  }
}
