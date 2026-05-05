fn main() {
  // Rerun proc macro to embed DB migrations if files change
  // https://docs.rs/diesel_migrations/latest/diesel_migrations/macro.embed_migrations.html
  println!("cargo:rerun-if-changed=migrations");

  // Bundle icons
  glib_build_tools::compile_resources(
    &["data"],
    "data/resources.gresource.xml",
    "resources.gresource",
  );
}
