// 极简 X11 窗口探测器（无 xwininfo 时的替代品）：
//   - 递归找 WM_NAME / WM_CLASS 含 dsh-pet 的窗口
//   - 打印几何（含相对屏幕的位置）、visual/depth、映射状态
//   - XGetImage 采样若干点，打印 ARGB（32 位 visual 下 A 在最高字节）
// 用法: ./xinspect [关键字]
#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>

static Display *dpy;
static const char *needle;
static int found = 0;

static void sample(Window w, int width, int height) {
  XImage *img = XGetImage(dpy, w, 0, 0, width, height, AllPlanes, ZPixmap);
  if (!img) {
    printf("    XGetImage 失败（窗口不可读）\n");
    return;
  }
  int pts[][2] = {{2, 2}, {width - 3, 2}, {2, height - 3}, {width - 3, height - 3},
                  {width / 2, height / 2}, {width / 2, height / 6}, {width / 6, height / 2}};
  const char *names[] = {"左上", "右上", "左下", "右下", "中心", "上中", "左中"};
  for (int i = 0; i < 7; i++) {
    int x = pts[i][0], y = pts[i][1];
    if (x < 0 || y < 0 || x >= width || y >= height) continue;
    unsigned long p = XGetPixel(img, x, y);
    unsigned a = (p >> 24) & 0xff, r = (p >> 16) & 0xff, g = (p >> 8) & 0xff, b = p & 0xff;
    printf("    %s(%d,%d) = #%02x%02x%02x%02x (A=%u R=%u G=%u B=%u)\n", names[i], x, y, a, r, g, b, a, r, g, b);
  }
  XDestroyImage(img);
}


// 打印子窗口（WebKit 可能把渲染面放在子 GdkWindow 里）并采样中心点
static void dump_children(Window w, int depth) {
  if (depth > 4) return;
  Window r, p, *kids = NULL;
  unsigned n = 0;
  if (!XQueryTree(dpy, w, &r, &p, &kids, &n)) return;
  for (unsigned i = 0; i < n; i++) {
    XWindowAttributes a;
    if (!XGetWindowAttributes(dpy, kids[i], &a)) continue;
    int x, y;
    Window c;
    XTranslateCoordinates(dpy, kids[i], w, 0, 0, &x, &y, &c);
    printf("    子窗口 0x%lx rel=(%d,%d) %dx%d depth=%d %s\n", (unsigned long)kids[i], x, y, a.width, a.height,
           a.depth, a.map_state == IsViewable ? "IsViewable" : "未映射");
    if (a.map_state == IsViewable && a.depth > 0) {
      XImage *im = XGetImage(dpy, kids[i], 0, 0, a.width, a.height, AllPlanes, ZPixmap);
      if (im) {
        unsigned long px = XGetPixel(im, a.width / 2, a.height / 2);
        printf("      中心=#%08lx (A=%lu R=%lu G=%lu B=%lu)\n", px, (px >> 24) & 0xff, (px >> 16) & 0xff,
               (px >> 8) & 0xff, px & 0xff);
        XDestroyImage(im);
      }
    }
    dump_children(kids[i], depth + 1);
  }
  if (kids) XFree(kids);
}

static void walk(Window w, int depth) {
  if (depth > 12) return;
  char *name = NULL;
  XClassHint ch;
  memset(&ch, 0, sizeof(ch));
  if (XFetchName(dpy, w, &name) && name) {
    if (strstr(name, needle)) {
      found++;
      Window root;
      int x, y;
      unsigned bw, d;
      XWindowAttributes attr;
      XGetWindowAttributes(dpy, w, &attr);
      Window child;
      XTranslateCoordinates(dpy, w, DefaultRootWindow(dpy), 0, 0, &x, &y, &child);
      XGetGeometry(dpy, w, &root, &x, &y, &attr.width, &attr.height, &bw, &d);
      XTranslateCoordinates(dpy, w, DefaultRootWindow(dpy), 0, 0, &x, &y, &child);
      printf("窗口 0x%lx 标题=%s\n", (unsigned long)w, name);
      printf("    位置=(%d,%d) 尺寸=%dx%d depth=%d visual=%p class=%d 映射状态=%s\n", x, y, attr.width,
             attr.height, attr.depth, (void *)attr.visual, attr.class,
             attr.map_state == IsViewable ? "IsViewable" : (attr.map_state == IsUnmapped ? "IsUnmapped" : "IsUnviewable"));
      if (XGetClassHint(dpy, w, &ch)) {
        printf("    WM_CLASS=%s / %s\n", ch.res_name ? ch.res_name : "", ch.res_class ? ch.res_class : "");
        if (ch.res_name) XFree(ch.res_name);
        if (ch.res_class) XFree(ch.res_class);
      }
      if (attr.map_state == IsViewable) { sample(w, attr.width, attr.height); dump_children(w, 0); }
    }
    XFree(name);
  }
  Window r, p, *kids = NULL;
  unsigned n = 0;
  if (XQueryTree(dpy, w, &r, &p, &kids, &n)) {
    for (unsigned i = 0; i < n; i++) walk(kids[i], depth + 1);
    if (kids) XFree(kids);
  }
}

int main(int argc, char **argv) {
  needle = argc > 1 ? argv[1] : "dsh-pet";
  dpy = XOpenDisplay(NULL);
  if (!dpy) {
    fprintf(stderr, "无法连接 X11\n");
    return 1;
  }
  printf("屏幕 %dx%d depth=%d\n", DisplayWidth(dpy, DefaultScreen(dpy)), DisplayHeight(dpy, DefaultScreen(dpy)),
         DefaultDepth(dpy, DefaultScreen(dpy)));
  walk(DefaultRootWindow(dpy), 0);
  if (!found) printf("未找到标题含 \"%s\" 的窗口\n", needle);
  XCloseDisplay(dpy);
  return found ? 0 : 2;
}
