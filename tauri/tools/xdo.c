// 极简 XTEST 输入注入（无 xdotool 时用）：move / click / down / up
#include <X11/Xlib.h>
#include <X11/extensions/XTest.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <X11/Xutil.h>

static Window find_win(Display *dpy, Window w, const char *needle) {
  char *name = NULL;
  if (XFetchName(dpy, w, &name) && name) {
    int hit = strcmp(name, needle) == 0;
    XFree(name);
    if (hit) return w;
  }
  Window r, p, *kids = NULL;
  unsigned n = 0;
  Window found = 0;
  if (XQueryTree(dpy, w, &r, &p, &kids, &n)) {
    for (unsigned i = 0; i < n && !found; i++) found = find_win(dpy, kids[i], needle);
    if (kids) XFree(kids);
  }
  return found;
}

int main(int argc, char **argv) {
  if (argc < 2) { fprintf(stderr, "用法: xdo move X Y | click [btn] | down [btn] | up [btn]\n"); return 2; }
  Display *d = XOpenDisplay(NULL);
  if (!d) { fprintf(stderr, "无法连接 X11\n"); return 1; }
  const char *cmd = argv[1];
  if (!strcmp(cmd, "movewin") && argc >= 5) {
    Window w = find_win(d, DefaultRootWindow(d), argv[2]);
    if (!w) { fprintf(stderr, "找不到窗口\n"); return 3; }
    XMoveWindow(d, w, atoi(argv[3]), atoi(argv[4]));
    printf("XMoveWindow 0x%lx -> %s,%s\n", (unsigned long)w, argv[3], argv[4]);
  } else if (!strcmp(cmd, "move") && argc >= 4) {
    XTestFakeMotionEvent(d, -1, atoi(argv[2]), atoi(argv[3]), 0);
  } else if (!strcmp(cmd, "down") || !strcmp(cmd, "up") || !strcmp(cmd, "click")) {
    int btn = argc >= 3 ? atoi(argv[2]) : 1;
    if (!strcmp(cmd, "click")) {
      XTestFakeButtonEvent(d, btn, True, 0); XFlush(d); usleep(60000);
      XTestFakeButtonEvent(d, btn, False, 0);
    } else {
      XTestFakeButtonEvent(d, btn, !strcmp(cmd, "down"), 0);
    }
  } else { fprintf(stderr, "不认识的命令\n"); XCloseDisplay(d); return 2; }
  XFlush(d);
  usleep(30000);
  XCloseDisplay(d);
  return 0;
}
