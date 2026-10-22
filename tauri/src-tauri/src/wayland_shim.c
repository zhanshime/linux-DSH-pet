// 原生 Wayland 的点击穿透：GTK3 没有输入形状 API（gdk_window_input_shape_combine_region 在
// Wayland 后端是空操作），但 Wayland 核心协议本身支持 —— wl_surface.set_input_region。
// 这里从 GDK 拿到 GTK 已经创建好的 wl_surface / wl_compositor（同一连接，不新开连接），
// 用 region 精确框出"可点区域"。空 region = 整窗穿透。
//
// 注意：region 是双缓冲状态，随下一次 surface commit 生效；调用方在设置后要 queue_draw
// 促使 GTK 尽快提交（视频逐帧渲染时天然就会提交）。
#include <gdk/gdkwayland.h>
#include <wayland-client.h>
#include <stdint.h>

// rects: [x, y, w, h] × count，坐标是 surface 本地逻辑像素（= 页面 CSS px）
// 返回 1 = 已应用；0 = 当前不是 Wayland 后端或拿不到对象
int dsh_pet_set_input_region(GdkWindow *gw, const int32_t *rects, int count) {
  if (!gw || !GDK_IS_WINDOW(gw)) return 0;
  GdkDisplay *dpy = gdk_window_get_display(gw);
  if (!GDK_IS_WAYLAND_DISPLAY(dpy)) return 0;
  struct wl_display *wl = gdk_wayland_display_get_wl_display(dpy);
  struct wl_compositor *comp = gdk_wayland_display_get_wl_compositor(dpy);
  struct wl_surface *surf = gdk_wayland_window_get_wl_surface(gw);
  if (!wl || !comp || !surf) return 0;

  struct wl_region *region = wl_compositor_create_region(comp);
  if (!region) return 0;
  for (int i = 0; i < count; i++) {
    int32_t x = rects[i * 4], y = rects[i * 4 + 1];
    int32_t w = rects[i * 4 + 2], h = rects[i * 4 + 3];
    if (w > 0 && h > 0) wl_region_add(region, x, y, w, h);
  }
  wl_surface_set_input_region(surf, region);
  wl_region_destroy(region);
  return 1;
}
