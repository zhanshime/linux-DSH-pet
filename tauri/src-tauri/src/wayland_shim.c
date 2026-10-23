// 原生 Wayland 的两个小补丁（GTK3 没有对应公开 API）：
//
//  1) 点击穿透：GTK3 的 Wayland 后端没实现输入形状 API，这里从 GDK 拿到 GTK 已创建好的
//     wl_surface / wl_compositor（同一连接，不新开），用 wl_surface.set_input_region 精确框出
//     "可点区域"。空 region = 整窗穿透。region 是双缓冲状态，随下一次 commit 生效。
//
//  2) xdg_toplevel_tag_v1：给窗口打一个稳定的 tag，供合成器/窗口规则（位置、尺寸、
//     always-on-top、用户规则）在应用重启后仍能识别同一个窗口。GTK3 不暴露 xdg_toplevel
//     这个 proxy，所以这里用符号插桩（interpose wl_proxy_marshal_array_flags）把 GTK 创建
//     的 xdg_toplevel 截下来；协议本身不提供"置顶"请求，它只是让规则能匹配到窗口。
#include <gdk/gdkwayland.h>
#include <wayland-client.h>
#include <dlfcn.h>
#include <stdint.h>
#include <string.h>

// ---------------------------------------------------------------------------
// 1) 点击穿透
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// 2) xdg_toplevel_tag_v1
// ---------------------------------------------------------------------------

// 手写最小客户端定义：协议头文件不在各发行版默认安装里，而客户端只需要两个请求。
// 事件为空；method 的 types 传 NULL（libwayland 不校验对象参数的具体接口）。
static const struct wl_message tag_manager_requests[] = {
    {"destroy", "", NULL},
    {"set_toplevel_tag", "os", NULL},
    {"set_toplevel_description", "os", NULL},
};

static const struct wl_interface tag_manager_interface = {
    "xdg_toplevel_tag_manager_v1", 1,
    (int) (sizeof(tag_manager_requests) / sizeof(tag_manager_requests[0])),
    tag_manager_requests,
    0, NULL,
};

// GTK 创建 xdg_toplevel 时截下来的 proxy（只有一块窗口，取第一个即可）
static struct wl_proxy *g_toplevel = NULL;

typedef struct wl_proxy *(*marshal_array_flags_fn)(struct wl_proxy *, uint32_t,
                                                   const struct wl_interface *,
                                                   uint32_t, uint32_t,
                                                   union wl_argument *);

// 符号插桩：GTK（以及我们自己）对 wl_proxy_marshal_array_flags 的调用都会走到这里。
// 生成代码里的 xdg_wm_base.get_xdg_toplevel 会带 interface=&xdg_toplevel_interface，
// 于是构造出来的 proxy 就能被识别并保存。
struct wl_proxy *wl_proxy_marshal_array_flags(struct wl_proxy *proxy, uint32_t opcode,
                                              const struct wl_interface *interface,
                                              uint32_t version, uint32_t flags,
                                              union wl_argument *args) {
  static marshal_array_flags_fn real = NULL;
  if (!real) {
    real = (marshal_array_flags_fn) dlsym(RTLD_NEXT, "wl_proxy_marshal_array_flags");
    if (!real) return NULL;
  }
  struct wl_proxy *result = real(proxy, opcode, interface, version, flags, args);
  if (result && interface && interface->name && !g_toplevel &&
      strcmp(interface->name, "xdg_toplevel") == 0) {
    g_toplevel = result;
  }
  return result;
}

static struct wl_proxy *g_tag_manager = NULL;
static struct wl_display *g_tag_manager_display = NULL;

static void tag_registry_global(void *data, struct wl_registry *registry, uint32_t name,
                                const char *interface, uint32_t version) {
  (void) data;
  (void) version;
  if (g_tag_manager || !interface) return;
  if (strcmp(interface, "xdg_toplevel_tag_manager_v1") != 0) return;
  g_tag_manager = wl_registry_bind(registry, name, &tag_manager_interface, 1);
}

static void tag_registry_global_remove(void *data, struct wl_registry *registry, uint32_t name) {
  (void) data;
  (void) registry;
  (void) name;
}

static const struct wl_registry_listener tag_registry_listener = {
    tag_registry_global,
    tag_registry_global_remove,
};

static int ensure_tag_manager(struct wl_display *wl) {
  if (g_tag_manager) return 1;
  if (g_tag_manager_display && g_tag_manager_display != wl) return 0;
  struct wl_registry *registry = wl_display_get_registry(wl);
  if (!registry) return 0;
  wl_registry_add_listener(registry, &tag_registry_listener, NULL);
  // 拿到 global 列表需要一次 roundtrip（主线程调用，阻塞很短）
  if (wl_display_roundtrip(wl) < 0) {
    wl_registry_destroy(registry);
    return 0;
  }
  wl_registry_destroy(registry);
  if (!g_tag_manager) return 0;
  g_tag_manager_display = wl;
  return 1;
}

// 返回 1 = 已下发；0 = 不是 Wayland / 没截到 toplevel / 合成器不支持该协议
int dsh_pet_set_toplevel_tag(GdkWindow *gw, const char *tag, const char *description) {
  if (!g_toplevel || !tag) return 0;
  if (!gw || !GDK_IS_WINDOW(gw)) return 0;
  GdkDisplay *dpy = gdk_window_get_display(gw);
  if (!GDK_IS_WAYLAND_DISPLAY(dpy)) return 0;
  struct wl_display *wl = gdk_wayland_display_get_wl_display(dpy);
  if (!wl || !ensure_tag_manager(wl)) return 0;

  // 请求号：1 = set_toplevel_tag(toplevel, tag)，2 = set_toplevel_description(toplevel, desc)
  wl_proxy_marshal_flags(g_tag_manager, 1, NULL, 1, 0, g_toplevel, tag);
  if (description) {
    wl_proxy_marshal_flags(g_tag_manager, 2, NULL, 1, 0, g_toplevel, description);
  }
  wl_display_flush(wl);
  return 1;
}
