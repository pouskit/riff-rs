//! Icône et métadonnées de version du `.exe` Windows. Sans effet hors Windows.

fn main() {
  println!("cargo:rerun-if-changed=build.rs");
  println!("cargo:rerun-if-changed=packaging/icons/icon.ico");
  #[cfg(windows)]
  if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows") {
    let mut resource = winresource::WindowsResource::new();
    resource
      .set_icon("packaging/icons/icon.ico")
      .set("ProductName", "Riff (natif)")
      .set("FileDescription", "Riff (natif)")
      .set("OriginalFilename", "riff-app.exe")
      .set("CompanyName", "Pouskit")
      .set("LegalCopyright", "Copyright (c) 2026 rootscripts, Pouskit");
    if let Err(error) = resource.compile() {
      panic!("échec de l'intégration des ressources Windows : {error}");
    }
  }
}
