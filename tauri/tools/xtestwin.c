// 验证 Mutter 对"部分出屏"的窗口怎么处理：普通 / USPosition 提示 / override-redirect
#include <X11/Xlib.h>
#include <X11/Xatom.h>
#include <X11/Xutil.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <unistd.h>

int main(int argc, char **argv) {
  const char *mode = argc > 1 ? argv[1] : "plain";
  int X = argc > 2 ? atoi(argv[2]) : 1203;
  int Y = argc > 3 ? atoi(argv[3]) : 489;
  Display *d = XOpenDisplay(NULL);
  if (!d) return 1;
  int s = DefaultScreen(d);
  Window root = RootWindow(d, s);
  XSetWindowAttributes a;
  memset(&a, 0, sizeof(a));
  a.override_redirect = strcmp(mode, "or") == 0;
  a.background_pixel = 0x3366cc;
  Window w = XCreateWindow(d, root, X, Y, 924, 744, 0, CopyFromParent, InputOutput, CopyFromParent,
                           CWOverrideRedirect | CWBackPixel, &a);
  XStoreName(d, w, "plain-test");
  if (strcmp(mode, "uspos") == 0) {
    XSizeHints h;
    memset(&h, 0, sizeof(h));
    h.flags = USPosition | USSize;
    h.x = X;
    h.y = Y;
    h.width = 924;
    h.height = 744;
    XSetWMNormalHints(d, w, &h);
  }
  XMapWindow(d, w);
  XFlush(d);
  sleep(2);
  if (strcmp(mode, "after") == 0) {
    XMoveWindow(d, w, X, Y);
    XRaiseWindow(d, w);
    XFlush(d);
    sleep(1);
  }
  if (strcmp(mode, "size") == 0) {
    XMoveResizeWindow(d, w, X, Y, 924, 744);
    XFlush(d);
    sleep(1);
  }
  XWindowAttributes at;
  XGetWindowAttributes(d, w, &at);
  Window r, parent, *kids = NULL;
  unsigned n = 0;
  XQueryTree(d, w, &r, &parent, &kids, &n);
  if (kids) XFree(kids);
  Window child;
  int ax, ay;
  XTranslateCoordinates(d, w, root, 0, 0, &ax, &ay, &child);
  printf("mode=%-6s 请求=%d,%d 属性=%d,%d 绝对=%d,%d parent=0x%lx\n", mode, X, Y, at.x, at.y, ax, ay,
         (unsigned long)parent);
  sleep(1);
  XDestroyWindow(d, w);
  XCloseDisplay(d);
  return 0;
}
