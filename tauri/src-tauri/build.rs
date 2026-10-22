fn main() {
    build_wayland_shim();
    tauri_build::build();
}

/// 编译点击穿透 shim：需要 GTK3/GDK 的 Wayland 头文件与 wayland-client。
/// 头文件路径/宏由 pkg-config 提供，避免写死发行版目录。
fn build_wayland_shim() {
    println!("cargo:rerun-if-changed=src/wayland_shim.c");
    // Fedora 的 /usr/lib64/ccache/gcc 包装器会往 ~/.cache/ccache 写缓存；在沙箱/只读 HOME 下
    // 会直接失败。构建脚本里显式关掉 ccache，改用真实 gcc。
    std::env::set_var("CCACHE_DISABLE", "1");
    let mut build = cc::Build::new();
    build.file("src/wayland_shim.c").warnings(false);
    if std::path::Path::new("/usr/bin/gcc").exists() {
        build.compiler("/usr/bin/gcc");
    }

    if let Ok(out) = std::process::Command::new("pkg-config")
        .args(["--cflags", "gtk+-3.0", "wayland-client"])
        .output()
    {
        if out.status.success() {
            for flag in String::from_utf8_lossy(&out.stdout).split_whitespace() {
                if let Some(inc) = flag.strip_prefix("-I") {
                    build.include(inc);
                } else if let Some(def) = flag.strip_prefix("-D") {
                    let (k, v) = def.split_once('=').unwrap_or((def, "1"));
                    build.define(k, Some(v));
                }
            }
        }
    }
    build.compile("dsh_pet_wayland_shim");

    // gdk_wayland_* 在 libgdk-3；wl_surface/wl_region 在 libwayland-client
    println!("cargo:rustc-link-lib=gdk-3");
    println!("cargo:rustc-link-lib=wayland-client");
}
