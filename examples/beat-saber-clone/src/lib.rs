use hotham::HothamResult;

#[cfg_attr(target_os = "android", ndk_glue::main(backtrace = "on"))]
pub fn main() {
    println!("[BEAT_SABER_EXAMPLE] MAIN!");
    real_main().expect("[BEAT_SABER_EXAMPLE] ERROR IN MAIN!");
}

pub fn real_main() -> HothamResult<()> {
    todo!();
    Ok(())
}
