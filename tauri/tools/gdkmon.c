#include <gtk/gtk.h>
static void dump(const char *tag, GdkMonitor *m) {
  if (!m) { printf("%s: null\n", tag); return; }
  GdkRectangle g, w;
  gdk_monitor_get_geometry(m, &g);
  gdk_monitor_get_workarea(m, &w);
  printf("%s: geom=%dx%d+%d+%d workarea=%dx%d+%d+%d scale=%d\n", tag,
         g.width, g.height, g.x, g.y, w.width, w.height, w.x, w.y,
         gdk_monitor_get_scale_factor(m));
}
int main(int argc, char **argv) {
  gtk_init(&argc, &argv);
  GdkDisplay *d = gdk_display_get_default();
  printf("backend=%s monitors=%d\n", G_OBJECT_TYPE_NAME(d), gdk_display_get_n_monitors(d));
  dump("monitor0", gdk_display_get_monitor(d, 0));
  dump("primary", gdk_display_get_primary_monitor(d));
  return 0;
}
